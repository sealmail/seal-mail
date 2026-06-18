#!/usr/bin/env node
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const docsDir = path.join(root, "docs");
const evalDir = path.join(docsDir, "ai-evaluation");
const outDir = path.join(root, "tmp", "ai-evaluation");
const factEvidencePath = path.join(outDir, "fact-evidence.json");

const requiredDocs = [
  "docs/product-philosophy.md",
  "docs/ai-testing-methodology.md",
  "docs/user-scenarios.md",
  "docs/ai-evaluation/rubric.md",
  "docs/ai-evaluation/schema.json",
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

function packetFiles() {
  if (!existsSync(evalDir)) {
    fail("missing docs/ai-evaluation directory");
  }
  return readdirSync(evalDir)
    .filter((name) => name.endsWith(".packet.md"))
    .sort()
    .map((name) => path.join(evalDir, name));
}

function validatePacket(file) {
  const text = readFileSync(file, "utf8");
  const requiredSections = [
    "Packet ID:",
    "Scenario ID:",
    "## 1. Product Philosophy Source",
    "## 2. User Persona",
    "## 3. Scenario",
    "## 4. Product Promise",
    "## 5. Evidence Inputs",
    "## 6. Rubric Focus",
    "## 7. Required Output",
    "## 8. Pass/Fail Rules",
  ];
  const missing = requiredSections.filter((section) => !text.includes(section));
  if (missing.length > 0) {
    fail(`${path.relative(root, file)} missing sections: ${missing.join(", ")}`);
  }
  if (!text.includes("docs/ai-evaluation/schema.json")) {
    fail(`${path.relative(root, file)} must reference schema.json`);
  }
  if (!text.includes("Output JSON only")) {
    fail(`${path.relative(root, file)} must require JSON-only output`);
  }
  return text;
}

function validateSchema() {
  const schemaText = readProjectFile("docs/ai-evaluation/schema.json");
  let schema;
  try {
    schema = JSON.parse(schemaText);
  } catch (error) {
    fail(`schema.json is not valid JSON: ${error.message}`);
  }
  const requiredTopLevel = [
    "scenarioId",
    "pass",
    "score",
    "summary",
    "dimensionScores",
    "findings",
    "evidenceCoverage",
    "reviewedAntiPatterns",
  ];
  for (const key of requiredTopLevel) {
    if (!schema.required?.includes(key)) {
      fail(`schema.json missing required key: ${key}`);
    }
  }
}

function commandValidate() {
  for (const file of requiredDocs) {
    readProjectFile(file);
  }
  validateSchema();
  const packets = packetFiles();
  if (packets.length === 0) {
    fail("no packet templates found under docs/ai-evaluation");
  }
  for (const file of packets) {
    validatePacket(file);
  }
  console.log(`AI evaluation assets OK: ${packets.length} packet(s)`);
}

function commandBuild(packetName) {
  if (!packetName) {
    fail("usage: bun run ai-eval:packet -- build <packet-file-name>");
  }
  commandValidate();
  const packetPath = path.join(evalDir, packetName);
  if (!existsSync(packetPath)) {
    fail(`packet not found: ${packetName}`);
  }
  const packet = validatePacket(packetPath);
  const evidence = existsSync(factEvidencePath)
    ? readFileSync(factEvidencePath, "utf8").trim()
    : [
        "Fill this section with collected evidence before sending to an AI evaluator.",
        "",
        "```json",
        "{",
        '  "factLayer": {},',
        '  "experienceLayer": {},',
        '  "knownConstraints": []',
        "}",
        "```",
      ].join("\n");
  const sections = [
    ["Product Philosophy", readProjectFile("docs/product-philosophy.md")],
    ["AI Testing Methodology", readProjectFile("docs/ai-testing-methodology.md")],
    ["User Scenarios", readProjectFile("docs/user-scenarios.md")],
    ["Rubric", readProjectFile("docs/ai-evaluation/rubric.md")],
    ["Output Schema", readProjectFile("docs/ai-evaluation/schema.json")],
    ["Scenario Packet", packet],
    ["Evidence", existsSync(factEvidencePath) ? ["```json", evidence, "```"].join("\n") : evidence],
  ];
  mkdirSync(outDir, { recursive: true });
  const outPath = path.join(outDir, packetName.replace(/\.packet\.md$/, ".compiled.md"));
  const body = sections
    .map(([title, content]) => `# ${title}\n\n${content.trim()}\n`)
    .join("\n---\n\n");
  writeFileSync(outPath, body);
  console.log(outPath);
}

const [command, arg] = process.argv.slice(2);

if (command === "validate" || command === undefined) {
  commandValidate();
} else if (command === "build") {
  commandBuild(arg);
} else {
  fail(`unknown command: ${command}`);
}
