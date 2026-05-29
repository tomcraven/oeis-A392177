use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::Resource;

use crate::index_order::VisitOrder;
use crate::model::GameDefinition;
use crate::sim::{OccupancyGrid, Simulation};

#[derive(Clone, Debug)]
pub struct SimDisplay {
    pub occupancy: OccupancyGrid,
    pub cursors: Vec<u32>,
    /// Turn-ordered log for hover placement paths (derived on the UI thread).
    pub placements: Arc<Vec<(u32, crate::model::PieceId)>>,
    pub turn_step: usize,
    /// Mirrors `Simulation::is_saturated` so the main thread can stop re-requesting advances once
    /// the sim has hit its memory ceiling (it cannot make further progress at this zoom).
    pub saturated: bool,
}

impl Default for SimDisplay {
    fn default() -> Self {
        Self {
            occupancy: OccupancyGrid::default(),
            cursors: Vec::new(),
            placements: Arc::new(Vec::new()),
            turn_step: 0,
            saturated: false,
        }
    }
}

fn display_from_sim(sim: &Simulation) -> SimDisplay {
    #[cfg(feature = "app_profile")]
    let start = std::time::Instant::now();
    let display = SimDisplay {
        occupancy: sim.occupancy.clone(),
        cursors: sim.cursors.clone(),
        placements: sim.placements.arc(),
        turn_step: sim.turn_step,
        saturated: sim.is_saturated(),
    };
    #[cfg(feature = "app_profile")]
    crate::app_profile::note_display_clone_ns(start.elapsed().as_nanos() as u64);
    display
}

// --- native: simulation on a background thread --------------------------------

#[cfg(not(target_family = "wasm"))]
mod threaded {
    use super::*;
    use bevy::platform::time::Instant;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Mutex, mpsc};
    use std::thread::{self, JoinHandle};

    enum SimCommand {
        Reset {
            def: GameDefinition,
            visit_order: VisitOrder,
        },
        Advance {
            target_index: u32,
            budget: Duration,
            job_id: u64,
        },
        Shutdown,
    }

    struct SimUpdate {
        display: SimDisplay,
        worker_idle: bool,
    }

    /// Main-thread facade; simulation runs on a dedicated worker thread.
    #[derive(Resource)]
    pub struct SimulationBridge {
        cmd_tx: Sender<SimCommand>,
        update_rx: Mutex<Receiver<SimUpdate>>,
        _worker: JoinHandle<()>,
        pub display: SimDisplay,
        visit_order: VisitOrder,
        advance_in_flight: bool,
        active_job_id: u64,
        next_job_id: u64,
    }

    impl SimulationBridge {
        pub fn spawn(def: GameDefinition, visit_order: VisitOrder) -> Self {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            let (update_tx, update_rx) = mpsc::channel();

            let worker = thread::Builder::new()
                .name("red_black_knights_sim".into())
                .spawn(move || sim_worker_main(cmd_rx, update_tx, def, visit_order))
                .expect("sim worker thread");

            Self {
                cmd_tx,
                update_rx: Mutex::new(update_rx),
                _worker: worker,
                display: SimDisplay::default(),
                visit_order,
                advance_in_flight: false,
                active_job_id: 0,
                next_job_id: 0,
            }
        }

        pub fn poll_updates(&mut self) -> bool {
            let mut updated = false;
            let Ok(rx) = self.update_rx.lock() else {
                return false;
            };
            loop {
                match rx.try_recv() {
                    Ok(update) => {
                        // Snapshots arrive in FIFO order and the simulation only ever advances
                        // forward (resets use job_id == u64::MAX), so every snapshot is valid,
                        // monotonic progress. Always apply it so rendering keeps updating even
                        // while the camera pans/zooms (which bumps `active_job_id` every frame).
                        self.display = update.display;
                        // `worker_idle` is emitted as the worker returns to its recv loop, so it
                        // always means "the worker is now idle". Clear the in-flight flag whenever
                        // we see it, regardless of job id: a stale job id here would otherwise
                        // leave us thinking the worker is busy forever, starving panning frames
                        // (which only request work via the `!is_busy()` branch) of new advances.
                        if update.worker_idle {
                            self.advance_in_flight = false;
                        }
                        updated = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.advance_in_flight = false;
                        break;
                    }
                }
            }
            updated
        }

        pub fn needs_work(&self, def: &GameDefinition, target_index: u32) -> bool {
            if self.display.cursors.is_empty() {
                return false;
            }
            self.display
                .cursors
                .iter()
                .enumerate()
                .any(|(id, &cursor)| {
                    def.pieces
                        .get(id)
                        .is_some_and(|a| a.enabled && cursor <= target_index)
                })
        }

        pub fn is_busy(&self) -> bool {
            self.advance_in_flight
        }

        /// True once the worker reported it hit the memory budget; callers should stop requesting
        /// advances (further progress is impossible at the current zoom until a reset).
        pub fn is_saturated(&self) -> bool {
            self.display.saturated
        }

        pub fn visit_order(&self) -> VisitOrder {
            self.visit_order
        }

        pub fn request_reset(&mut self, def: GameDefinition, visit_order: VisitOrder) {
            self.visit_order = visit_order;
            self.next_job_id += 1;
            self.advance_in_flight = false;
            let _ = self.cmd_tx.send(SimCommand::Reset { def, visit_order });
        }

        pub fn reprioritize_advance(&mut self, target_index: u32, budget: Duration) {
            self.next_job_id += 1;
            let job_id = self.next_job_id;
            self.active_job_id = job_id;
            self.advance_in_flight = true;
            let _ = self.cmd_tx.send(SimCommand::Advance {
                target_index,
                budget,
                job_id,
            });
        }

        pub fn request_advance(&mut self, target_index: u32, budget: Duration) {
            if self.advance_in_flight {
                return;
            }
            self.reprioritize_advance(target_index, budget);
        }
    }

    impl Drop for SimulationBridge {
        fn drop(&mut self) {
            let _ = self.cmd_tx.send(SimCommand::Shutdown);
        }
    }

    fn sim_worker_main(
        cmd_rx: Receiver<SimCommand>,
        update_tx: Sender<SimUpdate>,
        initial_def: GameDefinition,
        initial_order: VisitOrder,
    ) {
        let mut def = initial_def;
        let mut visit_order = initial_order;
        let mut sim = Simulation::new(&def, visit_order);
        let _ = update_tx.send(snapshot(&sim, true));

        while let Ok(command) = cmd_rx.recv() {
            match command {
                SimCommand::Shutdown => break,
                SimCommand::Reset {
                    def: new_def,
                    visit_order: new_order,
                } => {
                    def = new_def;
                    visit_order = new_order;
                    sim.visit_order = visit_order;
                    sim.reset(&def);
                    let _ = update_tx.send(snapshot(&sim, true));
                }
                SimCommand::Advance {
                    target_index,
                    budget,
                    job_id,
                } => {
                    run_advance_job(
                        &mut sim,
                        &mut def,
                        &cmd_rx,
                        &update_tx,
                        target_index,
                        budget,
                        job_id,
                    );
                }
            }
        }
    }

    fn run_advance_job(
        sim: &mut Simulation,
        def: &mut GameDefinition,
        cmd_rx: &Receiver<SimCommand>,
        update_tx: &Sender<SimUpdate>,
        mut target_index: u32,
        mut budget: Duration,
        mut job_id: u64,
    ) {
        loop {
            // Already at the memory ceiling: report idle without re-cloning the (large) backing
            // stores so the UI keeps the region filled so far.
            if sim.is_saturated() {
                let _ = update_tx.send(snapshot(sim, true));
                return;
            }
            sim.occupancy.ensure_unique_for_mutation();
            sim.placements.ensure_unique_for_mutation();
            let start = Instant::now();
            let mut turns = 0u32;

            while sim.needs_work(def, target_index) {
                if !sim.step_turn(def) {
                    break;
                }
                turns += 1;

                if turns % 512 == 0 {
                    if start.elapsed() >= budget || sim.mem_saturated() {
                        let _ = update_tx.send(snapshot(sim, true));
                        return;
                    }

                    if let Some(interrupt) = poll_interrupt(cmd_rx, job_id) {
                        let _ = update_tx.send(snapshot(sim, false));
                        match interrupt {
                            Interrupt::Advance {
                                target_index: t,
                                budget: b,
                                job_id: j,
                            } => {
                                target_index = t;
                                budget = b;
                                job_id = j;
                                continue;
                            }
                            Interrupt::Reset {
                                def: new_def,
                                visit_order: new_order,
                            } => {
                                *def = new_def;
                                sim.visit_order = new_order;
                                sim.reset(def);
                                let _ = update_tx.send(snapshot(sim, true));
                                return;
                            }
                            Interrupt::Shutdown => return,
                        }
                    }
                }
            }

            let _ = update_tx.send(snapshot(sim, true));
            return;
        }
    }

    enum Interrupt {
        Advance {
            target_index: u32,
            budget: Duration,
            job_id: u64,
        },
        Reset {
            def: GameDefinition,
            visit_order: VisitOrder,
        },
        Shutdown,
    }

    fn poll_interrupt(cmd_rx: &Receiver<SimCommand>, active_job_id: u64) -> Option<Interrupt> {
        let mut best: Option<Interrupt> = None;
        loop {
            match cmd_rx.try_recv() {
                Ok(SimCommand::Advance {
                    target_index,
                    budget,
                    job_id,
                }) => {
                    if job_id > active_job_id {
                        let replace = match &best {
                            None => true,
                            Some(Interrupt::Advance {
                                job_id: best_id, ..
                            }) => job_id > *best_id,
                            _ => true,
                        };
                        if replace {
                            best = Some(Interrupt::Advance {
                                target_index,
                                budget,
                                job_id,
                            });
                        }
                    }
                }
                Ok(SimCommand::Reset { def, visit_order }) => {
                    return Some(Interrupt::Reset { def, visit_order });
                }
                Ok(SimCommand::Shutdown) => return Some(Interrupt::Shutdown),
                Err(TryRecvError::Empty) => return best,
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }

    fn snapshot(sim: &Simulation, worker_idle: bool) -> SimUpdate {
        SimUpdate {
            display: display_from_sim(sim),
            worker_idle,
        }
    }
}

#[cfg(not(target_family = "wasm"))]
pub use threaded::SimulationBridge;

// --- wasm: simulation on the main thread (no std::thread) -------------------

#[cfg(target_family = "wasm")]
#[derive(Resource)]
pub struct SimulationBridge {
    sim: Simulation,
    def: GameDefinition,
    pub display: SimDisplay,
    advance_in_flight: bool,
    target_index: u32,
    budget: Duration,
    active_job_id: u64,
    next_job_id: u64,
}

#[cfg(target_family = "wasm")]
impl SimulationBridge {
    pub fn spawn(def: GameDefinition, visit_order: VisitOrder) -> Self {
        let sim = Simulation::new(&def, visit_order);
        let display = display_from_sim(&sim);
        Self {
            sim,
            def,
            display,
            advance_in_flight: false,
            target_index: 0,
            budget: Duration::ZERO,
            active_job_id: 0,
            next_job_id: 0,
        }
    }

    pub fn poll_updates(&mut self) -> bool {
        if !self.advance_in_flight {
            return false;
        }
        self.sim
            .advance_for_duration(&self.def, self.target_index, self.budget);
        self.display = display_from_sim(&self.sim);
        // A saturated sim cannot grow further; treat it like "no work left" so we stop polling
        // (and stop the per-frame occupancy/placement copy in `advance_for_duration`).
        if self.sim.is_saturated() || !self.sim.needs_work(&self.def, self.target_index) {
            self.advance_in_flight = false;
        }
        true
    }

    pub fn needs_work(&self, def: &GameDefinition, target_index: u32) -> bool {
        if self.display.cursors.is_empty() {
            return false;
        }
        self.display
            .cursors
            .iter()
            .enumerate()
            .any(|(id, &cursor)| {
                def.pieces
                    .get(id)
                    .is_some_and(|a| a.enabled && cursor <= target_index)
            })
    }

    pub fn is_busy(&self) -> bool {
        self.advance_in_flight
    }

    /// True once the sim hit the memory budget; callers should stop requesting advances (further
    /// progress is impossible at the current zoom until a reset).
    pub fn is_saturated(&self) -> bool {
        self.sim.is_saturated()
    }

    pub fn visit_order(&self) -> VisitOrder {
        self.sim.visit_order
    }

    pub fn request_reset(&mut self, def: GameDefinition, visit_order: VisitOrder) {
        self.next_job_id += 1;
        self.advance_in_flight = false;
        self.def = def;
        self.sim.visit_order = visit_order;
        self.sim.reset(&self.def);
        self.display = display_from_sim(&self.sim);
    }

    pub fn reprioritize_advance(&mut self, target_index: u32, budget: Duration) {
        self.next_job_id += 1;
        let job_id = self.next_job_id;
        self.active_job_id = job_id;
        self.advance_in_flight = true;
        self.target_index = target_index;
        self.budget = budget;
    }

    pub fn request_advance(&mut self, target_index: u32, budget: Duration) {
        if self.advance_in_flight {
            return;
        }
        self.reprioritize_advance(target_index, budget);
    }
}
