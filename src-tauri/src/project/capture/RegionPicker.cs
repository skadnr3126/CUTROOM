using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace Cutroom {
    public sealed class RegionPicker : Form {
        [DllImport("user32.dll")]
        static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
        [DllImport("dwmapi.dll")]
        static extern int DwmFlush();
        [DllImport("user32.dll")]
        static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int w, int h, uint flags);

        readonly Bitmap desktop;
        readonly Rectangle bounds;
        Point? anchor;
        Rectangle selection;

        RegionPicker(Bitmap desktop, Rectangle bounds) {
            this.desktop = desktop;
            this.bounds = bounds;
            AutoScaleMode = AutoScaleMode.None;
            FormBorderStyle = FormBorderStyle.None;
            StartPosition = FormStartPosition.Manual;
            Bounds = bounds;
            TopMost = true;
            ShowInTaskbar = false;
            DoubleBuffered = true;
            Cursor = Cursors.Cross;
            KeyPreview = true;
        }

        public static int SelectRegion(string output) {
            SetThreadDpiAwarenessContext(new IntPtr(-4)); // Per-monitor physical pixels.
            var bounds = SystemInformation.VirtualScreen;
            using (var desktop = new Bitmap(bounds.Width, bounds.Height, PixelFormat.Format32bppRgb)) {
                DwmFlush();
                using (var graphics = Graphics.FromImage(desktop))
                    graphics.CopyFromScreen(bounds.Location, Point.Empty, bounds.Size);
                using (var picker = new RegionPicker(desktop, bounds)) {
                    if (picker.ShowDialog() != DialogResult.OK) return 2;
                    SaveSelection(desktop, picker.selection, output);
                }
            }
            return 0;
        }

        public static void SaveSelection(Bitmap desktop, Rectangle selection, string output) {
            using (var crop = desktop.Clone(selection, desktop.PixelFormat))
            using (var file = new FileStream(output, FileMode.Truncate, FileAccess.Write, FileShare.ReadWrite | FileShare.Delete))
                crop.Save(file, ImageFormat.Png);
        }

        protected override void OnShown(EventArgs e) {
            base.OnShown(e);
            SetWindowPos(Handle, new IntPtr(-1), bounds.X, bounds.Y, bounds.Width, bounds.Height, 0);
            Activate();
        }

        public static Rectangle DragBounds(Point a, Point b, Size size) {
            var rect = Rectangle.FromLTRB(Math.Min(a.X, b.X), Math.Min(a.Y, b.Y),
                                          Math.Max(a.X, b.X), Math.Max(a.Y, b.Y));
            return Rectangle.Intersect(new Rectangle(Point.Empty, size), rect);
        }

        protected override void OnMouseDown(MouseEventArgs e) {
            if (e.Button == MouseButtons.Right) { DialogResult = DialogResult.Cancel; return; }
            if (e.Button == MouseButtons.Left) { anchor = e.Location; Capture = true; }
        }
        protected override void OnMouseMove(MouseEventArgs e) {
            if (!anchor.HasValue) return;
            selection = DragBounds(anchor.Value, e.Location, desktop.Size);
            Invalidate();
        }
        protected override void OnMouseUp(MouseEventArgs e) {
            if (e.Button != MouseButtons.Left || !anchor.HasValue) return;
            selection = DragBounds(anchor.Value, e.Location, desktop.Size);
            anchor = null;
            Capture = false;
            if (selection.Width > 0 && selection.Height > 0) DialogResult = DialogResult.OK;
            else Invalidate();
        }
        protected override void OnKeyDown(KeyEventArgs e) {
            if (e.KeyCode == Keys.Escape) DialogResult = DialogResult.Cancel;
        }
        protected override void OnDeactivate(EventArgs e) {
            base.OnDeactivate(e);
            if (Visible && DialogResult == DialogResult.None) DialogResult = DialogResult.Cancel;
        }
        protected override void OnPaint(PaintEventArgs e) {
            var g = e.Graphics;
            g.DrawImageUnscaled(desktop, 0, 0);
            using (var shade = new SolidBrush(Color.FromArgb(88, 0, 0, 0)))
                g.FillRectangle(shade, ClientRectangle);
            if (selection.Width > 0 && selection.Height > 0) {
                g.DrawImage(desktop, selection, selection, GraphicsUnit.Pixel);
                g.DrawRectangle(Pens.White, selection);
            } else {
                var monitor = Screen.FromPoint(Cursor.Position).Bounds;
                var at = new Point(monitor.Left - bounds.Left + 20, monitor.Top - bounds.Top + 20);
                TextRenderer.DrawText(g, "드래그하여 영역 선택   ·   Esc 취소", SystemFonts.MessageBoxFont,
                                      at, Color.White, Color.FromArgb(35, 35, 35));
            }
        }
    }
}
