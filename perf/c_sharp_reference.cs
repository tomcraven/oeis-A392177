
#if WITH_SKIA_SHARP
using SkiaSharp;
#endif

using System.Drawing;   // for Point class

namespace A392177 {
    /// <summary>
    /// A .NET 8.0 program to compute terms of sequences related to A392177.
    /// Define WITH_SKIA_SHARP and reference SkiaSharp for the Draw feature.
    /// </summary>
    internal class Program {
        /// <summary>
        /// Main entry point.
        /// </summary>
        /// <param name="args">Program arguments</param>
        /// <returns>0 in case of success, 1 otherwise</returns>
        static int Main(string[] args) {
            if (args.Length == 1 || args.Length > 4) {
                Console.WriteLine($"Usage: {nameof(A392177)} [black red [sequence [max]]]");
                Console.WriteLine($"Where:");
                Console.WriteLine("- black is the type of pieces used by the black player (default: Knight),");
                Console.WriteLine("- red is the type of pieces used by the red player (default: Knight),");
                Console.WriteLine("- sequence is the sequence to compute (default: Black),");
                Console.WriteLine("- max is the index of the last term (default: 10000).");
                Console.WriteLine();
                Console.WriteLine("Use '-' to keep a default value.");
                Console.WriteLine();
                Console.WriteLine("The available types of pieces are:");
                Console.WriteLine("- King,");
                Console.WriteLine("- Queen,");
                Console.WriteLine("- Rook,");
                Console.WriteLine("- Bishop,");
                Console.WriteLine("- Knight,");
                Console.WriteLine("- PawnNorth, PawnEast, PawnWest, PawnSouth,");
                Console.WriteLine("- Nightrider,");
                Console.WriteLine("- Wazir, Ferz, Dabbaba, Alfil,");
                Console.WriteLine("- Camel, Zebra, Giraffe.");
                Console.WriteLine();
                Console.WriteLine("The available types of sequences are:");
                Console.WriteLine("- Black: positions of black pieces (A392177),");
                Console.WriteLine("- Red: positions of red pieces (A392178),");
                Console.WriteLine("- White: unoccupied positions (A392179),");
                Console.WriteLine("- Race: # of blacks pieces minus # of red pieces (A392180),");
                Console.WriteLine("- Layout: +k/-k for the k-th black/red piece, 0 otherwise (A395486),");
                Console.WriteLine("- LayoutNorth: layout towards north,");
                Console.WriteLine("- LayoutEast: layout towards east,");
                Console.WriteLine("- LayoutWest: layout towards west,");
                Console.WriteLine("- LayoutSouth: layout towards south,");
                Console.WriteLine("- Occupation: occupation order (A395506),");
#if WITH_SKIA_SHARP
                Console.WriteLine("- Draw: save the spiral into the black-red-max.png file in the local folder,");
#endif

                Console.WriteLine("- BLackX and BlackY: X and Y coordinates of black pieces,");
                Console.WriteLine("- RedX and RedY: X and Y coordinates of red pieces.");
                Console.WriteLine();
                Console.WriteLine("Type names are not case sensitive.");

                return 0;
            } else {
                try {
                    var black = args.Length > 0 && args[0] != "-" ? BuildPlayer(args[0]) : new Knight();
                    var red = args.Length > 1 && args[1] != "-" ? BuildPlayer(args[1]) : new Knight();
                    using var sequence = args.Length > 2 && args[2] != "-" ? BuildSequence(args[2]) : new Black();
                    if (args.Length > 3 && args[3] != "-") {
                        if (int.TryParse(args[3], out var max) && max > 0) {
                            sequence.Max = max;
                        } else {
                            throw new ArgumentException($"Invalid count '{args[3]}'.");
                        }
                    }

                    sequence.Generate(black, red);

                    return 0;
                } catch (Exception ex) {
                    Console.Error.WriteLine($"Something went wrong (maybe use '{nameof(A392177)} help' for help):");
                    Console.Error.WriteLine(ex.ToString());

                    return 1;
                }
            }
        }

        /// <summary>
        /// Build a player.
        /// </summary>
        /// <param name="piece">The type of pieces used by that player</param>
        /// <returns>The corresponding player</returns>
        /// <exception cref="ArgumentException">When the piece is unknown</exception>
        private static Player BuildPlayer(string piece) {
            var playerType = typeof(Player).Assembly.GetType($"{nameof(A392177)}.{piece}", false, true);
            if (playerType != null
                && !playerType.IsAbstract
                && typeof(Player).IsAssignableFrom(playerType)) {
                var obj = Activator.CreateInstance(playerType);
                if (obj is Player player) {
                    return player;
                }
            }

            throw new ArgumentException($"Invalid piece type '{piece}'.");
        }

        /// <summary>
        /// Build a sequence generator.
        /// </summary>
        /// <param name="name">The sequence name</param>
        /// <returns>The corresponding sequence generator</returns>
        /// <exception cref="ArgumentException">The the generator is unknown</exception>
        private static Sequence BuildSequence(string name) {
            var sequenceType = typeof(Player).Assembly.GetType($"{nameof(A392177)}.{name}", false, true);
            if (sequenceType != null
                && !sequenceType.IsAbstract
                && typeof(Sequence).IsAssignableFrom(sequenceType)) {
                var obj = Activator.CreateInstance(sequenceType);
                if (obj is Sequence sequence) {
                    return sequence;
                }
            }

            throw new ArgumentException($"Invalid sequence type '{name}'.");
        }
    }

    /// <summary>
    /// Abstract sequence generator.
    /// </summary>
    /// <param name="n">The offset (by default: 1)</param>
    public abstract class Sequence(int n = 1) : IDisposable {
        /// <summary>
        /// The maximum index (by default: 10000).
        /// </summary>
        public int Max { get; set; } = 10_000;

        /// <summary>
        /// Generate the sequence for the given players.
        /// </summary>
        /// <param name="black">The black player</param>
        /// <param name="red">The red player</param>
        public virtual void Generate(Player black, Player red) {
            while (!this.Done) {
                this.Notice(black.Place(red), red.Place(black));
            }
        }

        public virtual void Dispose() { }

        /// <summary>
        /// Notice the next black and red positions.
        /// </summary>
        /// <param name="black">The black position</param>
        /// <param name="red">The red position</param>
        protected abstract void Notice(Point black, Point red);

        /// <summary>
        /// Emit a value.
        /// </summary>
        /// <param name="value">The value</param>
        protected virtual void Emit(int value) {
            if (n <= this.Max) {
                Console.WriteLine($"{n++} {value}");
            }

            if (n > this.Max) {
                this.Done = true;
            }
        }

        /// <summary>
        /// Done with this sequence?
        /// </summary>
        protected bool Done { get; set; }
    }

#if WITH_SKIA_SHARP
    /// <summary>
    /// Draw the spiral (as a .png file).
    /// </summary>
    public class Draw : Sequence {
        public override void Generate(Player black, Player red) {
            while (Spiral.Index(new(this.halfWidth, -this.halfWidth)) < this.Max) {
                this.halfWidth++;
            }
            this.bitmap = new SKBitmap(2 * this.halfWidth + 1, 2 * this.halfWidth + 1);
            {
                using var canvas = new SKCanvas(bitmap);
                canvas.DrawRect(0, 0, 2 * this.halfWidth + 1, 2 * this.halfWidth + 1, new SKPaint() {
                    Color = new SKColor(0xFFFFFFFF),
                });
            }
            base.Generate(black, red);

            var path =  $"{black.GetType().Name}-{red.GetType().Name}-{this.Max}.png";

            Console.WriteLine($"{path} {2 * this.halfWidth + 1}x{2 * this.halfWidth + 1}");

            using var stream = new FileStream(path, FileMode.Create, FileAccess.Write);
            using var image = SKImage.FromBitmap(bitmap);
            using var encodedImage = image.Encode();
            encodedImage.SaveTo(stream);
        }

        protected override void Notice(Point black, Point red) {
            var visibleBlack = this.Show(black, this.blackColor);
            var visibleRed = this.Show(red, this.redColor);
            if (!visibleBlack && !visibleRed) {
                this.Done = true;
            }
        }

        private bool Show(Point z, SKColor color) {
            if (Math.Max(Math.Abs(z.X), Math.Abs(z.Y)) <= this.halfWidth ) {
                this.bitmap?.SetPixel(this.halfWidth + z.X, this.halfWidth - z.Y, color);
                return true;
            } else {
                return false;
            }
        }

        public override void Dispose() {
            this.bitmap?.Dispose();
            base.Dispose();
        }

        private SKBitmap? bitmap;
        private int halfWidth;
        private SKColor redColor = new SKColor(0xFFFF0000);
        private SKColor blackColor = new SKColor(0xFF000000);
    }
#endif

    /// <summary>
    /// Sequence of black positions.
    /// </summary>
    public class Black : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(Spiral.Index(black));
        }
    }

    /// <summary>
    /// Sequence of black X-coordinates.
    /// </summary>
    public class BlackX : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(black.X);
        }
    }

    /// <summary>
    /// Sequence of black Y-coordinates.
    /// </summary>
    public class BlackY : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(black.Y);
        }
    }

    /// <summary>
    /// Sequence of red X-coordinates.
    /// </summary>
    public class RedX : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(red.X);
        }
    }

    /// <summary>
    /// Sequence of red Y-coordinates.
    /// </summary>
    public class RedY : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(red.Y);
        }
    }

    /// <summary>
    /// Sequence of red positions.
    /// </summary>
    public class Red : Sequence {
        protected override void Notice(Point black, Point red) {
            this.Emit(Spiral.Index(red));
        }
    }

    /// <summary>
    /// Sequence of unoccupied positions.
    /// </summary>
    public class White : Sequence {
        protected override void Notice(Point black, Point red) {
            int limit = Math.Min(Spiral.Index(black), Spiral.Index(red));
            while (Spiral.Index(this.z) < limit) {
                if (!Player.Occupied(this.z)) {
                    this.Emit(Spiral.Index(this.z));
                }
                this.z = Spiral.Move(this.z);
            }
        }

        private Point z = Point.Empty;
    }

    /// <summary>
    /// Sequence of pieces alongside some half axis.
    /// </summary>
    public abstract class LayoutHalfAxis(Func<Point, int> func) : Sequence(0) {
        protected override void Notice(Point black, Point red) {
            this.axis ??= new int[1 + this.Max];

            this.n++;

            var blackAxis = func(black);
            if (blackAxis >= 0 && blackAxis < this.axis.Length) {
                this.axis[blackAxis] = +this.n;
            }

            var redAxis = func(red);
            if (redAxis >= 0 && redAxis < this.axis.Length) {
                this.axis[redAxis] = -this.n;
            }

            var w = Math.Min(
                Math.Max(Math.Abs(black.X), Math.Abs(black.Y)),
                Math.Max(Math.Abs(red.X), Math.Abs(red.Y))
            );

            while (this.k <= w && this.k < this.axis.Length) {
                this.Emit(this.axis[this.k++]);
            }
        }

        protected static readonly Func<Point, int> north = z => z.X == 0 && z.Y >= 0 ? +z.Y : -1;
        protected static readonly Func<Point, int> east  = z => z.Y == 0 && z.X >= 0 ? +z.X : -1;
        protected static readonly Func<Point, int> west  = z => z.Y == 0 && z.X <= 0 ? -z.X : -1;
        protected static readonly Func<Point, int> south = z => z.X == 0 && z.Y <= 0 ? -z.Y : -1;

        private int[]? axis;
        private int k = 0;
        private int n = 0;
    }

    public class LayoutNorth : LayoutHalfAxis { public LayoutNorth() : base(north) { } }
    public class LayoutEast : LayoutHalfAxis { public LayoutEast() : base(east) { } }
    public class LayoutWest : LayoutHalfAxis { public LayoutWest() : base(west) { } }
    public class LayoutSouth : LayoutHalfAxis { public LayoutSouth() : base(south) { } }

    /// <summary>
    /// Sequence of pieces encoded as positive/negative values for black/red pieces.
    /// </summary>
    public class Layout : Sequence {
        public Layout() : base(0) { }
        protected override void Notice(Point black, Point red) {
            this.spiral ??= new int[1 + this.Max];

            this.n++;

            var blackIndex = Spiral.Index(black);
            if (blackIndex < this.spiral.Length) {
                this.spiral[blackIndex] = +this.n;
            }

            var redIndex = Spiral.Index(red);
            if (redIndex < this.spiral.Length) {
                this.spiral[redIndex] = -this.n;
            }

            int limit = Math.Min(blackIndex, redIndex);

            while (this.k <= limit && this.k < this.spiral.Length) {
                this.Emit(this.spiral[k++]);
            }
        }

        private int[]? spiral = null;
        private int k = 0;
        private int n = 0;
    }

    /// <summary>
    /// Occupation order.
    /// </summary>
    public class Occupation : Sequence {
        protected override void Notice(Point black, Point red) {
            this.occupation[black] = ++this.n;
            this.occupation[red] = ++this.n;

            int min = Math.Min(Spiral.Index(black), Spiral.Index(red));
            while (Spiral.Index(this.z) <= min) {
                if (this.occupation.TryGetValue(this.z, out var value)) {
                    this.Emit(value);
                }
                this.z = Spiral.Move(this.z);
            }
        }

        private readonly Dictionary<Point, int> occupation = [];
        private int n = 0;
        private Point z = Point.Empty;
    }

    /// <summary>
    /// Number of blacks pieces minus number of red pieces.
    /// </summary>
    public class Race : Layout {
        protected override void Emit(int value) {
            this.race += Math.Sign(value);
            base.Emit(this.race);
        }

        private int race = 0;
    }

    /// <summary>
    /// Spiral board.
    /// </summary>
    internal static class Spiral {
        /// <summary>
        /// Move to the next point (further from the origin) alongside the square spiral.
        /// </summary>
        /// <param name="z">The current point</param>
        /// <returns>The next point</returns>
        public static Point Move(Point z) {
            int w = Math.Max(Math.Abs(z.X), Math.Abs(z.Y));
            if (z.Y == -w) return z + new Size(1, 0);
            if (z.X == -w) return z + new Size(0, -1);
            if (z.Y == w) return z + new Size(-1, 0);
            return z + new Size(0, 1);
        }

        /// <summary>
        /// O-base index of a point on the square spiral.
        /// </summary>
        /// <param name="z">The point</param>
        /// <returns>Its index</returns>
        public static int Index(Point z) {
            int x = z.X, y = z.Y;
            if (x > Math.Abs(y)) return (2 * x - 1) * (2 * x - 1) + (x + y - 1);
            if (x > y) return 2 * -y * (1 - 2 * y) + (x - y);
            if (y >= Math.Abs(x)) return 2 * y * (2 * y - 1) + (y - x);
            return (4 * x * x) + (-y - x);
        }
    }

    /// <summary>
    /// A player.
    /// </summary>
    public abstract class Player {
        /// <summary>
        /// Is this player attacking the given position?
        /// </summary>
        /// <param name="z">The position</param>
        /// <returns>Attacking?</returns>
        public abstract bool IsAttacking(Point z);

        /// <summary>
        /// Place a piece a some position.
        /// </summary>
        /// <param name="z">The positioni</param>
        public abstract void AttackFrom(Point z);

        /// <summary>
        /// Place the next piece.
        /// </summary>
        /// <param name="opponent">The other player</param>
        /// <returns>The position of the next piece</returns>
        public Point Place(Player opponent) {
            while (Occupied(this.z0) || opponent.IsAttacking(this.z0)) {
                this.z0 = Spiral.Move(this.z0);
            }

            Occupy(this.z0);
            AttackFrom(this.z0);

            return this.z0;
        }

        /// <summary>
        /// Next starting point (avoiding occupied or attacked cells).
        /// </summary>
        private Point z0 = Point.Empty;

        /// <summary>
        /// Is this position occupied?
        /// </summary>
        /// <param name="z">The position to check</param>
        /// <returns>Occupied?</returns>
        internal static bool Occupied(Point z) {
            return occupied.Contains(z);
        }

        /// <summary>
        /// Occupy the given position.
        /// </summary>
        /// <param name="z">The position to occupy</param>
        protected static void Occupy(Point z) {
            occupied.Add(z);
        }

        /// <summary>
        /// Occupied cells.
        /// </summary>
        private static readonly HashSet<Point> occupied = [];
    }

    /// <summary>
    /// A simple player (with finite moves).
    /// </summary>
    /// <param name="moves">The corresponding moves</param>
    public abstract class SimplePlayer(params Size[] moves) : Player {
        public override void AttackFrom(Point z) {
            foreach (var move in moves) {
                this.attacked.Add(z + move);
            }
        }
        public override bool IsAttacking(Point z) {
            return this.attacked.Contains(z);
        }

        /// <summary>
        /// Cells attacked by this player.
        /// </summary>
        private readonly HashSet<Point> attacked = [];
    }

    /// <summary>
    /// Pawn attacking northward.
    /// </summary>
    public class PawnNorth : SimplePlayer { public PawnNorth() : base(new Size(0, +1)) { } }

    /// <summary>
    /// Pawn attacking eastward.
    /// </summary>
    public class PawnEast : SimplePlayer { public PawnEast() : base(new Size(+1, 0)) { } }

    /// <summary>
    /// Pawn attacking westward.
    /// </summary>
    public class PawnWest : SimplePlayer { public PawnWest() : base(new Size(-1, 0)) { } }

    /// <summary>
    /// Pawn attacking southward.
    /// </summary>
    public class PawnSouth : SimplePlayer { public PawnSouth() : base(new Size(0, -1)) { } }

    /// <summary>
    /// Knight.
    /// </summary>
    public class Knight : SimplePlayer {
        public Knight() : base(Moves) {}

        private static readonly Size[] Moves = {
                         new(-1, +2), new(+1, +2),
            new(-2, +1),                           new(+2, +1),
            new(-2, -1),                           new(+2, -1),
                         new(+1, -2), new(-1, -2),
        };
    }

    /// <summary>
    /// Camel.
    /// </summary>
    public class Camel : SimplePlayer {
        public Camel() : base(Moves) { }

        private static readonly Size[] Moves = {
                         new(-1, +3), new(+1, +3),
            new(-3, +1),                           new(+3, +1),
            new(-3, -1),                           new(+3, -1),
                         new(+1, -3), new(-1, -3),
        };
    }

    /// <summary>
    /// Giraffe.
    /// </summary>
    public class Giraffe : SimplePlayer {
        public Giraffe() : base(Moves) { }

        private static readonly Size[] Moves = {
                         new(-1, +4), new(+1, +4),
            new(-4, +1),                           new(+4, +1),
            new(-4, -1),                           new(+4, -1),
                         new(+1, -4), new(-1, -4),
        };
    }

    /// <summary>
    /// Zebra.
    /// </summary>
    public class Zebra : SimplePlayer {
        public Zebra() : base(Moves) { }

        private static readonly Size[] Moves = {
                         new(-2, +3), new(+2, +3),
            new(-3, +2),                           new(+3, +2),
            new(-3, -2),                           new(+3, -2),
                         new(+2, -3), new(-2, -3),
        };
    }

    /// <summary>
    /// King.
    /// </summary>
    public class King : SimplePlayer {
        public King() : base(Moves) {}

        private static readonly Size[] Moves = {
            new(-1, +1), new(0, +1), new(+1, +1),
            new(-1,  0),             new(+1,  0),
            new(-1, -1), new(0, -1), new(+1, -1),
        };
    }

    /// <summary>
    /// Wazir.
    /// </summary>
    public class Wazir : SimplePlayer {
        public Wazir() : base(Moves) {}

        private static readonly Size[] Moves = {
                         new(0, +1),
            new(-1,  0),             new(+1,  0),
                         new(0, -1),
        };
    }

    /// <summary>
    /// Dabbaba.
    /// </summary>
    public class Dabbaba : SimplePlayer {
        public Dabbaba() : base(Moves) { }

        private static readonly Size[] Moves = {
                         new(0, +2),
            new(-2,  0),             new(+2,  0),
                         new(0, -2),
        };
    }

    /// <summary>
    /// Ferz.
    /// </summary>
    public class Ferz : SimplePlayer {
        public Ferz() : base(Moves) {}

        private static readonly Size[] Moves = {
            new(-1, +1), new(+1, +1),
            new(-1, -1), new(+1, -1),
        };
    }

    /// <summary>
    /// Alfil.
    /// </summary>
    public class Alfil : SimplePlayer {
        public Alfil() : base(Moves) { }

        private static readonly Size[] Moves = {
            new(-2, +2), new(+2, +2),
            new(-2, -2), new(+2, -2),
        };
    }

    /// <summary>
    /// Parallel lines.
    /// </summary>
    /// <param name="mx">The X-coordinate multiplier</param>
    /// <param name="my">The Y-coordinate multiplier</param>
    public class ParallelLines(int mx, int my) {
        /// <summary>
        /// Control the line passing through the given point.
        /// </summary>
        /// <param name="z">The point to control</param>
        public void Control(Point z) {
            this.lines.Add(this.Index(z));
        }

        /// <summary>
        /// Is the given point on a line controlled?
        /// </summary>
        /// <param name="z">The point to check</param>
        /// <returns>Controlled?</returns>
        public bool Controlled(Point z) {
            return this.lines.Contains(this.Index(z));
        }
        private int Index(Point z) {
            return mx * z.X + my * z.Y;
        }

        private readonly HashSet<int> lines = [];
    }

    /// <summary>
    /// Horizontal lines.
    /// </summary>
    internal class HorizontalLines : ParallelLines { public HorizontalLines() : base(1, 0) {} }
    /// <summary>
    /// Vertical lines.
    /// </summary>
    internal class VerticalLines : ParallelLines { public VerticalLines() : base(0, 1) {} }
    /// <summary>
    /// Diagonal lines.
    /// </summary>
    internal class DiagonalLines : ParallelLines { public DiagonalLines() : base(1, -1) {} }
    /// <summary>
    /// Antidiagonal lines.
    /// </summary>
    internal class AntiDiagonalLines : ParallelLines { public AntiDiagonalLines() : base(1, 1) {} }

    /// <summary>
    /// A complex layer (with infinite moves alongside lines).
    /// </summary>
    /// <param name="lines">The corresopnding line directions</param>
    public abstract class ComplexPlayer(ParallelLines[] lines) : Player {
        public override void AttackFrom(Point z) {
            foreach (var line in this.lines) {
                line.Control(z);
            }
        }
        public override bool IsAttacking(Point z) {
            return this.lines.Any(ray => ray.Controlled(z));
        }

        private readonly ParallelLines[] lines = lines;
    }

    /// <summary>
    /// Queen.
    /// </summary>
    public class Queen : ComplexPlayer {
        public Queen() : base([new HorizontalLines(), new VerticalLines(), new DiagonalLines(), new AntiDiagonalLines()]) { }
    }

    /// <summary>
    /// Rook.
    /// </summary>
    public class Rook : ComplexPlayer {
        public Rook() : base([new HorizontalLines(), new VerticalLines()]) { }
    }

    /// <summary>
    /// Bishop.
    /// </summary>
    public class Bishop : ComplexPlayer {
        public Bishop() : base([new DiagonalLines(), new AntiDiagonalLines()]) { }
    }

    /// <summary>
    /// Nightrider.
    /// </summary>
    public class NightRider : ComplexPlayer {
        public NightRider() : base([
            new(1,2), new (1,-2),
            new(2,1), new (2,-1)
        ]) { }
    }
}