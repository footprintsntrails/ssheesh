// Real mouse and keyboard, and a watch on a window's focus and whether it answers.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

static class Win {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  delegate void EventProc(IntPtr hook, uint ev, IntPtr h, int obj, int child, uint thread, uint time);
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public UIntPtr w; public IntPtr l; public uint t; public POINT p; }
  [StructLayout(LayoutKind.Sequential)] struct GUITHREADINFO { public int cb; public uint flags; public IntPtr active, focus, capture, menu, move, caret; public RECT r; }
  [DllImport("user32")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32")] static extern bool SetCursorPos(int x, int y);
  [DllImport("user32")] static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32")] static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32")] static extern short VkKeyScan(char c);
  [DllImport("user32")] static extern uint MapVirtualKey(uint c, uint t);
  [DllImport("user32")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32")] static extern bool GetGUIThreadInfo(uint t, ref GUITHREADINFO i);
  [DllImport("user32")] static extern bool IsHungAppWindow(IntPtr h);
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32")] static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, IntPtr l, uint f, uint ms, out UIntPtr r);
  [DllImport("user32")] static extern IntPtr SetWinEventHook(uint a, uint b, IntPtr m, EventProc p, uint pid, uint t, uint f);
  [DllImport("user32")] static extern bool PeekMessage(out MSG m, IntPtr h, uint a, uint b, uint r);
  [DllImport("user32")] static extern bool TranslateMessage(ref MSG m);
  [DllImport("user32")] static extern IntPtr DispatchMessage(ref MSG m);
  [DllImport("user32")] static extern bool SetProcessDPIAware();
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern IntPtr LoadKeyboardLayout(string id, uint f);
  [DllImport("user32")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);

  static string Cls(IntPtr h) { var s = new StringBuilder(128); GetClassName(h, s, 128); return s.ToString(); }
  static string Title(IntPtr h) { var s = new StringBuilder(128); GetWindowText(h, s, 128); return s.ToString(); }
  static string J(string s) { return "\"" + (s ?? "").Replace("\\", "\\\\").Replace("\"", "'") + "\""; }

  static IntPtr MainWindow(int pid) {
    IntPtr best = IntPtr.Zero; long area = 0;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        RECT r; GetWindowRect(h, out r);
        long a = (long)(r.R - r.L) * (r.B - r.T);
        if (a > area) { area = a; best = h; }
      }
      return true;
    }, IntPtr.Zero);
    return best;
  }

  static void Key(byte vk, bool up) { keybd_event(vk, (byte)MapVirtualKey(vk, 0), up ? 2u : 0u, UIntPtr.Zero); }
  static void Tap(byte vk) { Key(vk, false); Thread.Sleep(15); Key(vk, true); Thread.Sleep(15); }

  static void Type(string text) {
    foreach (char c in text) {
      if (c == '\n') { Tap(0x0D); continue; }
      short k = VkKeyScan(c);
      byte vk = (byte)(k & 0xFF); bool shift = (k & 0x100) != 0;
      if (shift) Key(0x10, false);
      Tap(vk);
      if (shift) Key(0x10, true);
    }
  }

  // Set from the event hook, read by the watch below.
  static int focusEvents, fgEvents;
  static Dictionary<string, int> focusBy = new Dictionary<string, int>();
  static EventProc keep;

  static void Watch(int pid, string path, string stop) {
    keep = (hook, ev, h, obj, child, thread, time) => {
      if (ev == 3) { fgEvents++; return; }
      focusEvents++;
      string c = Cls(h);
      int n; focusBy.TryGetValue(c, out n); focusBy[c] = n + 1;
    };
    SetWinEventHook(3, 3, IntPtr.Zero, keep, 0, 0, 0);
    SetWinEventHook(0x8005, 0x8005, IntPtr.Zero, keep, 0, 0, 0);
    var proc = Process.GetProcessById(pid);
    var cpuAt = proc.TotalProcessorTime; var sec = Stopwatch.StartNew();
    IntPtr polled = IntPtr.Zero; int polledChanges = 0; long nextPing = 0, nextLine = 1000;
    long pingMax = 0; int pingFail = 0, hung = 0;
    using (var w = new StreamWriter(path) { AutoFlush = true }) {
      var all = Stopwatch.StartNew();
      while (!File.Exists(stop)) {
        MSG m;
        while (PeekMessage(out m, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref m); DispatchMessage(ref m); }
        var info = new GUITHREADINFO(); info.cb = Marshal.SizeOf(typeof(GUITHREADINFO));
        if (GetGUIThreadInfo(0, ref info) && info.focus != polled) { polled = info.focus; polledChanges++; }
        IntPtr main = MainWindow(pid);
        if (all.ElapsedMilliseconds >= nextPing && main != IntPtr.Zero) {
          nextPing = all.ElapsedMilliseconds + 200;
          UIntPtr r; var t = Stopwatch.StartNew();
          if (SendMessageTimeout(main, 0, UIntPtr.Zero, IntPtr.Zero, 0, 2000, out r) == IntPtr.Zero) pingFail++;
          pingMax = Math.Max(pingMax, t.ElapsedMilliseconds);
          if (IsHungAppWindow(main)) hung++;
        }
        if (all.ElapsedMilliseconds >= nextLine) {
          nextLine += 1000;
          proc.Refresh();
          double cpu = 0;
          try { var c = proc.TotalProcessorTime; cpu = (c - cpuAt).TotalMilliseconds / sec.Elapsed.TotalMilliseconds * 100; cpuAt = c; sec.Restart(); } catch { }
          IntPtr fg = GetForegroundWindow();
          string by = string.Join(",", focusBy.OrderByDescending(x => x.Value).Take(5).Select(x => J(x.Key) + ":" + x.Value).ToArray());
          w.WriteLine("{\"at\":" + DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() + ",\"focus\":" + focusEvents + ",\"fg\":" + fgEvents + ",\"polled\":" + polledChanges + ",\"pingMax\":" + pingMax + ",\"pingFail\":" + pingFail + ",\"hung\":" + hung + ",\"cpu\":" + Math.Round(cpu) + ",\"isApp\":" + (fg == main ? "true" : "false") + ",\"fgClass\":" + J(Cls(fg)) + ",\"fgTitle\":" + J(Title(fg)) + ",\"focusClass\":" + J(Cls(polled)) + ",\"by\":{" + by + "}}");
          focusEvents = fgEvents = polledChanges = 0; pingMax = 0; pingFail = 0; hung = 0; focusBy.Clear();
        }
        Thread.Sleep(3);
      }
    }
  }

  static int Main(string[] a) {
    SetProcessDPIAware();
    switch (a[0]) {
      case "origin": {
        IntPtr h = MainWindow(int.Parse(a[1]));
        if (h == IntPtr.Zero) { Console.WriteLine("none"); return 1; }
        POINT p = new POINT(); ClientToScreen(h, ref p); RECT r; GetWindowRect(h, out r);
        Console.WriteLine(p.X + " " + p.Y + " " + (r.R - r.L) + " " + (r.B - r.T) + " " + Cls(h));
        return 0;
      }
      case "click": {
        SetCursorPos(int.Parse(a[1]), int.Parse(a[2])); Thread.Sleep(60);
        mouse_event(2, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(40); mouse_event(4, 0, 0, 0, UIntPtr.Zero);
        return 0;
      }
      case "keys": Type(a[1].Replace("{n}", "\n")); return 0;
      case "alttab": {
        Key(0x12, false); Thread.Sleep(30); Tap(0x09); Thread.Sleep(30); Key(0x12, true);
        return 0;
      }
      case "newterm": { Key(0x11, false); Key(0x10, false); Tap(0x54); Key(0x10, true); Key(0x11, true); return 0; }
      case "press": {
        bool ctrl = a.Contains("ctrl"), shift = a.Contains("shift"), noPlace = a.Contains("noplace");
        Action<byte, bool> press = (vk, up) => { if (noPlace) keybd_event(vk, 0, up ? 2u : 0u, UIntPtr.Zero); else Key(vk, up); };
        byte key = Convert.ToByte(a[a.Length - 1], 16);
        if (ctrl) press(0x11, false);
        if (shift) press(0x10, false);
        press(key, false); Thread.Sleep(15); press(key, true); Thread.Sleep(15);
        if (shift) press(0x10, true);
        if (ctrl) press(0x11, true);
        return 0;
      }
      case "layout": {
        // As the language bar asks for it: of the window that has the keys.
        IntPtr layout = LoadKeyboardLayout(a[1], 1);
        var info = new GUITHREADINFO(); info.cb = Marshal.SizeOf(typeof(GUITHREADINFO));
        if (GetGUIThreadInfo(0, ref info) && info.focus != IntPtr.Zero) PostMessage(info.focus, 0x50, IntPtr.Zero, layout);
        PostMessage(GetForegroundWindow(), 0x50, IntPtr.Zero, layout);
        return 0;
      }
      case "shot": {
        var b = System.Windows.Forms.Screen.PrimaryScreen.Bounds;
        using (var bmp = new Bitmap(b.Width, b.Height))
        using (var g = Graphics.FromImage(bmp)) { g.CopyFromScreen(b.Location, Point.Empty, b.Size); bmp.Save(a[1], ImageFormat.Png); }
        return 0;
      }
      case "watch": Watch(int.Parse(a[1]), a[2], a[3]); return 0;
    }
    return 2;
  }
}
