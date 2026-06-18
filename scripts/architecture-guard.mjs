#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();

const platformInvokeAllowList = new Set([
  "check_for_update",
  "cli_json",
  "ledger_get_addresses",
  "oauth_begin_browser",
  "oauth_begin_device",
  "oauth_finish_browser",
  "oauth_poll_device",
  "open_external_url",
  "open_pending_notification_mail",
]);

const businessInvokeDenyList = new Set([
  "add_account",
  "archive_message",
  "apply_filters",
  "bind_ledger",
  "create_folder",
  "delete_draft",
  "delete_filter",
  "delete_folder",
  "delete_message",
  "get_close_behavior",
  "get_message",
  "get_notify_new_mail",
  "get_state",
  "list_cached",
  "list_contacts",
  "list_drafts",
  "list_folders",
  "list_thread",
  "mark_read",
  "move_message",
  "remove_account",
  "remove_trusted",
  "save_attachment",
  "save_draft",
  "save_filter",
  "send_mail",
  "set_close_behavior",
  "set_flagged",
  "set_notify_new_mail",
  "set_read",
  "sync_messages",
  "sync_older_messages",
  "test_connection",
  "trust_sender",
  "use_local_key",
]);

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

function findInvokes(relativePath) {
  const text = readProjectFile(relativePath);
  const invokes = [];
  const invokePattern = /invoke(?:<[^>]+>)?\(\s*["']([^"']+)["']/g;
  for (let match = invokePattern.exec(text); match; match = invokePattern.exec(text)) {
    const line = text.slice(0, match.index).split("\n").length;
    invokes.push({ command: match[1], line });
  }
  return invokes;
}

function validateFrontendInvokeBoundary() {
  const files = ["src/api.ts", "src/url.ts", "src/updater.ts"];
  const invokes = files.flatMap((file) => findInvokes(file).map((invoke) => ({ file, ...invoke })));

  if (!invokes.some((invoke) => invoke.file === "src/api.ts" && invoke.command === "cli_json")) {
    fail("src/api.ts must expose business operations through cli_json");
  }

  for (const invoke of invokes) {
    if (businessInvokeDenyList.has(invoke.command)) {
      fail(`${invoke.file}:${invoke.line} directly invokes business command "${invoke.command}"; route it through cli_json`);
      continue;
    }
    if (!platformInvokeAllowList.has(invoke.command)) {
      fail(`${invoke.file}:${invoke.line} invokes non-whitelisted Tauri command "${invoke.command}"`);
    }
  }
}

function validateCliEntrypoints() {
  const main = readProjectFile("src-tauri/src/main.rs");
  if (!main.includes("SEALMAIL_RUN_CLI")) {
    fail("src-tauri/src/main.rs must keep the SEALMAIL_RUN_CLI app-bundle CLI entrypoint");
  }
  if (!main.includes("sealmail_lib::cli::main_entry()")) {
    fail("src-tauri/src/main.rs must call sealmail_lib::cli::main_entry() for CLI mode");
  }

  const standalone = readProjectFile("src-tauri/src/bin/sealmail-cli.rs");
  if (!standalone.includes("sealmail_lib::cli::main_entry()")) {
    fail("src-tauri/src/bin/sealmail-cli.rs must be a thin wrapper around sealmail_lib::cli::main_entry()");
  }
}

validateFrontendInvokeBoundary();
validateCliEntrypoints();

if (process.exitCode) {
  process.exit(process.exitCode);
}

console.log("Architecture guard OK: GUI business operations are routed through cli_json");
