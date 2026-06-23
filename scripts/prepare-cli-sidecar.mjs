import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const tauriDir = join(root, "src-tauri");
const target = process.env.SEALMAIL_SIDECAR_TARGET || hostTriple();
const isWindows = target.includes("windows");
const source = join(
  tauriDir,
  "target",
  process.env.SEALMAIL_SIDECAR_TARGET ? target : "",
  "release",
  isWindows ? "sealmail-cli.exe" : "sealmail-cli",
);
const destDir = join(tauriDir, "binaries");
const dest = join(destDir, `sealmail-cli-${target}${isWindows ? ".exe" : ""}`);

const cargoArgs = ["build", "--manifest-path", join(tauriDir, "Cargo.toml"), "--release", "--bin", "sealmail-cli"];
if (process.env.SEALMAIL_SIDECAR_TARGET) {
  cargoArgs.push("--target", target);
}

mkdirSync(destDir, { recursive: true });
if (!existsSync(dest)) {
  writeFileSync(dest, "");
  chmodSync(dest, 0o755);
}
execFileSync("cargo", cargoArgs, { cwd: root, stdio: "inherit" });
copyFileSync(source, dest);
console.log(`Prepared SealMail CLI sidecar: ${dest}`);

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { cwd: root, encoding: "utf8" });
  const line = out.split("\n").find((entry) => entry.startsWith("host: "));
  if (!line) {
    throw new Error("Unable to detect Rust host triple");
  }
  return line.slice("host: ".length).trim();
}
