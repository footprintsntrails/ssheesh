import { describe, expect, it } from "vitest";
import { pasteQuestion } from "./upload";

const file = (path: string, isDir = false) => ({ path, isDir });

describe("pasteQuestion", () => {
  it("says what's going and where, in full", () => {
    const q = pasteQuestion([file("/home/me/report.pdf"), file("/home/me/photos", true)], "web-1", "/var/www");
    expect(q.title).toBe("Upload these 2 items to web-1?");
    expect(q.body).toContain("/home/me/report.pdf");
    expect(q.body).toContain("/home/me/photos (folder, with all that's in it)");
    expect(q.body).toContain("Into /var/www");
    expect(q.risky).toBe(false);
    expect(q.confirmLabel).toBe("Upload");
    expect(pasteQuestion([file("/a")], "web-1", "/").title).toBe("Upload this to web-1?");
  });

  it("warns about what looks private", () => {
    for (const path of ["/home/me/.ssh", "/home/me/.ssh/id_ed25519", "C:\\Users\\me\\.ssh\\id_rsa", "/app/.env", "/app/.env.production", "/certs/server.key", "/certs/site.pem", "/home/me/.aws/credentials", "/home/me/passwords.kdbx", "/home/me/.kube/config"]) {
      const q = pasteQuestion([file(path)], "web-1", "/tmp");
      expect(q.risky, path).toBe(true);
      expect(q.body, path).toContain("looks private");
      expect(q.confirmLabel).toBe("Upload anyway");
    }
    for (const path of ["/home/me/report.pdf", "/home/me/keyboard.png", "/home/me/environment.txt", "/home/me/secretary.doc"]) {
      expect(pasteQuestion([file(path)], "web-1", "/tmp").risky, path).toBe(false);
    }
  });

  it("names a few and counts the rest", () => {
    const many = Array.from({ length: 9 }, (_, i) => file(`/f${i}`));
    const q = pasteQuestion(many, "web-1", "/tmp");
    expect(q.body).toContain("/f5");
    expect(q.body).not.toContain("/f6");
    expect(q.body).toContain("and 3 more");
  });

  it("hides names when asked to, as in streamer mode", () => {
    const q = pasteQuestion([file("/home/me/.ssh")], "web-1", "/srv", () => "•••");
    expect(q.body).not.toContain("/home/me");
    expect(q.body).not.toContain("/srv");
  });
});
