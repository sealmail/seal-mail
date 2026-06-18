#!/usr/bin/env node
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const outDir = path.join(root, "tmp", "ai-evaluation");

const coreUserCapabilities = [
  { name: "sync mail", cliCommands: ["sync", "sync-older"], guiExports: ["syncMessages", "syncOlderMessages"], scenarios: ["S02"] },
  { name: "list inbox", cliCommands: ["list"], guiExports: ["listCached"], scenarios: ["S02"] },
  { name: "read message", cliCommands: ["read", "thread"], guiExports: ["getMessage", "listThread"], scenarios: ["S03"] },
  { name: "send mail", cliCommands: ["send"], guiExports: ["sendMail"], scenarios: ["S04"] },
  { name: "draft mail", cliCommands: ["draft save", "draft delete", "drafts"], guiExports: ["saveDraft", "deleteDraft", "listDrafts"], scenarios: ["S05"] },
  { name: "attachments", cliCommands: ["attachment save"], guiExports: ["saveAttachment"], scenarios: ["S06"] },
  {
    name: "organize mail",
    cliCommands: ["move", "archive", "delete", "mark", "flag", "folder create", "folder delete"],
    guiExports: ["moveMessage", "archiveMessage", "deleteMessage", "setRead", "markRead", "setFlagged", "createFolder", "deleteFolder"],
    scenarios: ["S07"],
  },
  { name: "trusted mail", cliCommands: ["trust add", "trust remove", "trusted"], guiExports: ["trustSender", "removeTrusted"], scenarios: ["S08", "S09", "S10", "S11"] },
];

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function readProjectFile(relativePath) {
  const fullPath = path.join(root, relativePath);
  if (!existsSync(fullPath)) {
    fail(`missing required file: ${relativePath}`);
  }
  return readFileSync(fullPath, "utf8");
}

function gitValue(args) {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function hasCliCommand(cliSource, command) {
  const words = command.split(" ");
  if (words.length === 1) {
    return cliSource.includes(`"${command}" =>`) && cliSource.includes(`  ${command}`);
  }
  return words.every((word) => cliSource.includes(`"${word}"`)) && cliSource.includes(`  ${command}`);
}

function exportedFunctions(apiSource) {
  const names = [];
  const pattern = /export async function ([A-Za-z0-9_]+)/g;
  for (let match = pattern.exec(apiSource); match; match = pattern.exec(apiSource)) {
    names.push(match[1]);
  }
  return names.sort();
}

function directInvokes(source, file) {
  const invokes = [];
  const pattern = /invoke(?:<[^>]+>)?\(\s*["']([^"']+)["']/g;
  for (let match = pattern.exec(source); match; match = pattern.exec(source)) {
    const line = source.slice(0, match.index).split("\n").length;
    invokes.push({ file, line, command: match[1] });
  }
  return invokes;
}

function scenarioIds(source) {
  return [...source.matchAll(/^### (S\d+)/gm)].map((match) => match[1]);
}

function packetNames() {
  const dir = path.join(root, "docs", "ai-evaluation");
  if (!existsSync(dir)) return [];
  return readdirSync(dir).filter((name) => name.endsWith(".packet.md")).sort();
}

function collect() {
  const cliSource = readProjectFile("src-tauri/src/cli.rs");
  const apiSource = readProjectFile("src/api.ts");
  const urlSource = readProjectFile("src/url.ts");
  const updaterSource = readProjectFile("src/updater.ts");
  const scenarios = readProjectFile("docs/user-scenarios.md");

  const apiExports = exportedFunctions(apiSource);
  const scenariosFound = scenarioIds(scenarios);

  const capabilities = coreUserCapabilities.map((capability) => ({
    name: capability.name,
    cliCommands: capability.cliCommands.map((command) => ({
      command,
      present: hasCliCommand(cliSource, command),
    })),
    guiExports: capability.guiExports.map((name) => ({
      name,
      present: apiExports.includes(name),
    })),
    scenarios: capability.scenarios.map((id) => ({
      id,
      present: scenariosFound.includes(id),
    })),
  }));

  const evidence = {
    generatedAt: new Date().toISOString(),
    git: {
      branch: gitValue(["branch", "--show-current"]),
      commit: gitValue(["rev-parse", "--short", "HEAD"]),
      dirty: gitValue(["status", "--short"]) !== "",
    },
    architecture: {
      appBundleCliEntrypoint: readProjectFile("src-tauri/src/main.rs").includes("SEALMAIL_RUN_CLI"),
      standaloneCliEntrypoint: readProjectFile("src-tauri/src/bin/sealmail-cli.rs").includes("sealmail_lib::cli::main_entry()"),
      directInvokes: [
        ...directInvokes(apiSource, "src/api.ts"),
        ...directInvokes(urlSource, "src/url.ts"),
        ...directInvokes(updaterSource, "src/updater.ts"),
      ],
    },
    cli: {
      commandCount: capabilities.reduce((sum, item) => sum + item.cliCommands.length, 0),
      helpMentionsJsonMode: cliSource.includes("[--json]"),
    },
    gui: {
      exportedFunctionCount: apiExports.length,
      usesCliJson: apiSource.includes('invoke<T>("cli_json"'),
    },
    docs: {
      scenarioIds: scenariosFound,
      packetNames: packetNames(),
    },
    coreUserCapabilities: capabilities,
  };

  mkdirSync(outDir, { recursive: true });
  const outPath = path.join(outDir, "fact-evidence.json");
  writeFileSync(outPath, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(outPath);
}

collect();
