#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();

const coreUserCapabilities = [
  {
    name: "sync mail",
    cliCommands: ["sync", "sync-older"],
    guiExports: ["syncMessages", "syncOlderMessages"],
    scenarios: ["S02"],
  },
  {
    name: "list inbox",
    cliCommands: ["list"],
    guiExports: ["listCached"],
    scenarios: ["S02"],
  },
  {
    name: "read message",
    cliCommands: ["read", "thread"],
    guiExports: ["getMessage", "listThread"],
    scenarios: ["S03"],
  },
  {
    name: "send mail",
    cliCommands: ["send"],
    guiExports: ["sendMail"],
    scenarios: ["S04"],
  },
  {
    name: "draft mail",
    cliCommands: ["draft save", "draft delete", "drafts"],
    guiExports: ["saveDraft", "deleteDraft", "listDrafts"],
    scenarios: ["S05"],
  },
  {
    name: "attachments",
    cliCommands: ["attachment save", "attachment data"],
    guiExports: ["saveAttachment", "readAttachment"],
    scenarios: ["S06"],
  },
  {
    name: "organize mail",
    cliCommands: ["move", "archive", "delete", "mark", "flag", "folder create", "folder delete"],
    guiExports: ["moveMessage", "archiveMessage", "deleteMessage", "setRead", "markRead", "setFlagged", "createFolder", "deleteFolder"],
    scenarios: ["S07"],
  },
  {
    name: "trusted mail",
    cliCommands: ["trust add", "trust remove", "trusted"],
    guiExports: ["trustSender", "removeTrusted"],
    scenarios: ["S08", "S09", "S10", "S11"],
  },
];

function fail(message) {
  console.error(`error: ${message}`);
  process.exitCode = 1;
}

function readProjectFile(relativePath) {
  const fullPath = path.join(root, relativePath);
  if (!existsSync(fullPath)) {
    fail(`missing required file: ${relativePath}`);
    return "";
  }
  return readFileSync(fullPath, "utf8");
}

function hasCliCommand(cliSource, command) {
  const words = command.split(" ");
  if (words.length === 1) {
    return cliSource.includes(`"${command}" =>`) && cliSource.includes(`  ${command}`);
  }
  return words.every((word) => cliSource.includes(`"${word}"`)) && cliSource.includes(`  ${command}`);
}

function validateCliSurface() {
  const cliSource = readProjectFile("src-tauri/src/cli.rs");
  for (const capability of coreUserCapabilities) {
    for (const command of capability.cliCommands) {
      if (!hasCliCommand(cliSource, command)) {
        fail(`CLI missing core user command "${command}" for ${capability.name}`);
      }
    }
  }
}

function validateGuiSurface() {
  const apiSource = readProjectFile("src/api.ts");
  for (const capability of coreUserCapabilities) {
    for (const exportName of capability.guiExports) {
      if (!apiSource.includes(`export async function ${exportName}`)) {
        fail(`GUI API missing ${exportName} for ${capability.name}`);
      }
    }
  }
}

function validateScenarioCoverage() {
  const scenarios = readProjectFile("docs/user-scenarios.md");
  for (const capability of coreUserCapabilities) {
    for (const scenario of capability.scenarios) {
      if (!scenarios.includes(`### ${scenario}`) && !scenarios.includes(`## ${scenario}`)) {
        fail(`docs/user-scenarios.md missing ${scenario} coverage for ${capability.name}`);
      }
    }
  }
}

validateCliSurface();
validateGuiSurface();
validateScenarioCoverage();

if (process.exitCode) {
  process.exit(process.exitCode);
}

console.log(`Functional completeness guard OK: ${coreUserCapabilities.length} core user capabilities covered`);
