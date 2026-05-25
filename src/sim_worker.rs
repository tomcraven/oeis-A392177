use std::sync::{mpsc, Mutex};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bevy::prelude::Resource;

use crate::model::GameDefinition;
use crate::sim::{OccupancyGrid, Simulation};

#[derive(Clone, Debug, Default)]
pub struct SimDisplay {
    pub occupancy: OccupancyGrid,
    pub cursors: Vec<u32>,
    pub placements_len: usize,
    pub turn_step: usize,
}

enum SimCommand {
    Reset(GameDefinition),
    Advance {
        target_index: u32,
        budget: Duration,
        job_id: u64,
    },
    Shutdown,
}

struct SimUpdate {
    display: SimDisplay,
    job_id: u64,
    /// Worker returned to the command loop (time slice or job finished).
    worker_idle: bool,
}

/// Main-thread facade; simulation runs on a dedicated worker thread.
#[derive(Resource)]
pub struct SimulationBridge {
    cmd_tx: Sender<SimCommand>,
    update_rx: Mutex<Receiver<SimUpdate>>,
    _worker: JoinHandle<()>,
    pub display: SimDisplay,
    advance_in_flight: bool,
    active_job_id: u64,
    next_job_id: u64,
}

impl SimulationBridge {
    pub fn spawn(def: GameDefinition) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();

        let worker = thread::Builder::new()
            .name("red_black_knights_sim".into())
            .spawn(move || sim_worker_main(cmd_rx, update_tx, def))
            .expect("sim worker thread");

        Self {
            cmd_tx,
            update_rx: Mutex::new(update_rx),
            _worker: worker,
            display: SimDisplay::default(),
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
                    if update.job_id >= self.active_job_id {
                        self.display = update.display;
                    }
                    if update.worker_idle && update.job_id >= self.active_job_id {
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

    pub fn needs_work(&self, target_index: u32) -> bool {
        self.display
            .cursors
            .iter()
            .any(|&cursor| cursor <= target_index)
    }

    pub fn is_busy(&self) -> bool {
        self.advance_in_flight
    }

    pub fn request_reset(&mut self, def: GameDefinition) {
        self.next_job_id += 1;
        self.advance_in_flight = false;
        let _ = self.cmd_tx.send(SimCommand::Reset(def));
    }

    /// Queue simulation work. Supersedes any in-flight advance when `target_index` or scheduling changes.
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

fn sim_worker_main(cmd_rx: Receiver<SimCommand>, update_tx: Sender<SimUpdate>, initial_def: GameDefinition) {
    let mut def = initial_def;
    let mut sim = Simulation::new(&def);
    let _ = update_tx.send(snapshot(&sim, 0, true));

    while let Ok(command) = cmd_rx.recv() {
        match command {
            SimCommand::Shutdown => break,
            SimCommand::Reset(new_def) => {
                def = new_def;
                sim.reset(&def);
                let _ = update_tx.send(snapshot(&sim, u64::MAX, true));
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

/// Runs until the time budget elapses or a newer `Advance`/`Reset` arrives on the channel.
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
        let start = Instant::now();
        let mut turns = 0u32;

        while sim.needs_work(target_index) {
            if !sim.step_turn(def) {
                break;
            }
            turns += 1;

            if turns % 512 == 0 {
                if start.elapsed() >= budget {
                    let _ = update_tx.send(snapshot(sim, job_id, true));
                    return;
                }

                if let Some(interrupt) = poll_interrupt(cmd_rx, job_id) {
                    let _ = update_tx.send(snapshot(sim, job_id, false));
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
                        Interrupt::Reset(new_def) => {
                            *def = new_def;
                            sim.reset(def);
                            let _ = update_tx.send(snapshot(sim, u64::MAX, true));
                            return;
                        }
                        Interrupt::Shutdown => return,
                    }
                }
            }
        }

        let _ = update_tx.send(snapshot(sim, job_id, true));
        return;
    }
}

enum Interrupt {
    Advance {
        target_index: u32,
        budget: Duration,
        job_id: u64,
    },
    Reset(GameDefinition),
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
                        Some(Interrupt::Advance { job_id: best_id, .. }) => job_id > *best_id,
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
            Ok(SimCommand::Reset(def)) => return Some(Interrupt::Reset(def)),
            Ok(SimCommand::Shutdown) => return Some(Interrupt::Shutdown),
            Err(TryRecvError::Empty) => return best,
            Err(TryRecvError::Disconnected) => return None,
        }
    }
}

fn snapshot(sim: &Simulation, job_id: u64, worker_idle: bool) -> SimUpdate {
    SimUpdate {
        display: SimDisplay {
            occupancy: sim.occupancy.clone(),
            cursors: sim.cursors.clone(),
            placements_len: sim.placements.len(),
            turn_step: sim.turn_step,
        },
        job_id,
        worker_idle,
    }
}
