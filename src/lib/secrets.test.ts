import { describe, expect, it } from "vitest";
import { findSecrets, keyLines, keyOpenAfter, maskSecrets } from "./secrets";

/** The secret parts of a line, as text. */
const secrets = (line: string) => findSecrets(line).map((s) => line.slice(s.start, s.end));

describe("finding secrets in output", () => {
  it("knows tokens by their shape", () => {
    const gh = `ghp_${"a1B2".repeat(9)}`;
    expect(secrets(`token is ${gh} ok`)).toEqual([gh]);
    expect(secrets("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123")).toEqual(["sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"]);
    expect(secrets("export OPENAI=sk-proj-AbCdEfGhIjKlMnOpQrStUv")).toEqual(["sk-proj-AbCdEfGhIjKlMnOpQrStUv"]);
    expect(secrets("AKIAIOSFODNN7EXAMPLE")).toEqual(["AKIAIOSFODNN7EXAMPLE"]);
    expect(secrets(`bot 1234567890:AA${"x".repeat(33)} started`)).toEqual([`1234567890:AA${"x".repeat(33)}`]);
    const jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    expect(secrets(`Authorization: Bearer ${jwt}`)).toEqual([jwt]);
  });

  it("covers the value, not the name", () => {
    expect(secrets("DB_PASSWORD=hunter22")).toEqual(["hunter22"]);
    expect(secrets("  api_key: 'with spaces in it'")).toEqual(["with spaces in it"]);
    expect(secrets('{"client_secret": "abcd1234"}')).toEqual(["abcd1234"]);
    expect(secrets("GITHUB_TOKEN = abc123def")).toEqual(["abc123def"]);
    expect(secrets("DATABASE_URL=postgres://app:s3cr3t@db:5432/app")).toEqual(["s3cr3t"]);
  });

  it("still finds a password in a URL that streamer mode dotted out in part", () => {
    // A server whose user is "postgres": streaming hides that word, scheme and all.
    expect(secrets("DATABASE_URL=••••••••://app:s3cr3t@db:5432/app")).toEqual(["s3cr3t"]);
    expect(secrets("url: my••••••://••••:s3cr3t@db/app")).toEqual(["s3cr3t"]);
  });

  it("leaves ordinary output alone", () => {
    for (const line of [
      "max_tokens: 4096",
      "token_count: 12",
      "PasswordAuthentication no",
      "[sudo] password for deploy:",
      "PASSWORD=true",
      "api_key: ${API_KEY}",
      "commit 3f9a2c1e8b7d6a5f4e3d2c1b0a9f8e7d6c5b4a39",
      "https://example.com/docs/tokens",
      "Bearer",
    ]) {
      expect(secrets(line), line).toEqual([]);
    }
  });
});

describe("private keys", () => {
  // A made-up key, its markers put together here so no scanner takes the file for a real one.
  const kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
  const key = [`-----BEGIN ${kind}-----`, "NotARealKeyJustLettersAndDigits0123456789abcdefghijklmnopqrstuvwxyzAB", "MoreMadeUpLettersHere", `-----END ${kind}-----`];

  it("covers the lines between BEGIN and END", () => {
    expect(keyLines(["$ cat id_ed25519", ...key, "$ "])).toEqual([false, false, true, true, false, false]);
  });

  it("knows a key goes on from above the screen", () => {
    expect(keyOpenAfter(key.slice(0, 2))).toBe(true);
    expect(keyOpenAfter(key)).toBe(false);
    expect(keyLines(key.slice(2), true)).toEqual([true, false]);
  });

  it("masks text shown elsewhere", () => {
    expect(maskSecrets("ok\nTOKEN=abcdef\n" + key.join("\n"))).toBe(`ok\nTOKEN=••••••\n${key[0]}\n${"•".repeat(key[1].length)}\n${"•".repeat(key[2].length)}\n${key[3]}`);
  });
});
