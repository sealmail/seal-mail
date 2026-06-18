#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const outDir = path.join(root, "tmp", "ai-evaluation");
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
const requiredDimensions = [
  "coreTaskClarity",
  "basicReliability",
  "featureRestraint",
  "stateHonesty",
  "errorActionability",
  "cognitiveLoad",
  "evidenceSufficiency",
];

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function projectPath(relativePath) {
  return path.join(root, relativePath);
}

function assertObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
}

function assertString(value, label) {
  if (typeof value !== "string" || value.trim().length === 0) {
    fail(`${label} must be a non-empty string`);
  }
}

function assertBoolean(value, label) {
  if (typeof value !== "boolean") {
    fail(`${label} must be a boolean`);
  }
}

function assertIntegerInRange(value, label, min, max) {
  if (!Number.isInteger(value) || value < min || value > max) {
    fail(`${label} must be an integer between ${min} and ${max}`);
  }
}

function assertStringArray(value, label) {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    fail(`${label} must be an array of strings`);
  }
}

function parseJsonFile(file) {
  if (!file) {
    fail("usage: bun run ai-eval:run -- validate-result <result-json-file>");
  }
  const fullPath = path.isAbsolute(file) ? file : projectPath(file);
  if (!existsSync(fullPath)) {
    fail(`missing result file: ${file}`);
  }
  return parseJson(readFileSync(fullPath, "utf8"), file);
}

function parseJson(text, label) {
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(`${label} must be valid JSON: ${error.message}`);
  }
}

function validateDimensionScore(value, label) {
  assertObject(value, label);
  const allowed = new Set(["score", "reason"]);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) fail(`${label}.${key} is not allowed`);
  }
  assertIntegerInRange(value.score, `${label}.score`, 0, 5);
  assertString(value.reason, `${label}.reason`);
}

function validateFinding(value, label) {
  assertObject(value, label);
  const allowed = new Set(["severity", "principle", "issue", "evidence", "recommendation"]);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) fail(`${label}.${key} is not allowed`);
  }
  if (!["P0", "P1", "P2", "P3"].includes(value.severity)) {
    fail(`${label}.severity must be one of P0, P1, P2, P3`);
  }
  assertString(value.principle, `${label}.principle`);
  assertString(value.issue, `${label}.issue`);
  assertString(value.evidence, `${label}.evidence`);
  assertString(value.recommendation, `${label}.recommendation`);
}

function validateResult(result) {
  assertObject(result, "result");

  const allowedTopLevel = new Set([...requiredTopLevel, "openQuestions"]);
  for (const key of Object.keys(result)) {
    if (!allowedTopLevel.has(key)) fail(`result.${key} is not allowed`);
  }
  for (const key of requiredTopLevel) {
    if (!(key in result)) fail(`result.${key} is required`);
  }

  assertString(result.scenarioId, "result.scenarioId");
  assertBoolean(result.pass, "result.pass");
  assertIntegerInRange(result.score, "result.score", 0, 100);
  assertString(result.summary, "result.summary");

  assertObject(result.dimensionScores, "result.dimensionScores");
  for (const key of Object.keys(result.dimensionScores)) {
    if (!requiredDimensions.includes(key)) fail(`result.dimensionScores.${key} is not allowed`);
  }
  for (const key of requiredDimensions) {
    validateDimensionScore(result.dimensionScores[key], `result.dimensionScores.${key}`);
  }

  if (!Array.isArray(result.findings)) {
    fail("result.findings must be an array");
  }
  result.findings.forEach((finding, index) => validateFinding(finding, `result.findings[${index}]`));

  assertObject(result.evidenceCoverage, "result.evidenceCoverage");
  const allowedEvidenceKeys = new Set(["factLayer", "experienceLayer", "missingEvidence"]);
  for (const key of Object.keys(result.evidenceCoverage)) {
    if (!allowedEvidenceKeys.has(key)) fail(`result.evidenceCoverage.${key} is not allowed`);
  }
  assertStringArray(result.evidenceCoverage.factLayer, "result.evidenceCoverage.factLayer");
  assertStringArray(result.evidenceCoverage.experienceLayer, "result.evidenceCoverage.experienceLayer");
  assertStringArray(result.evidenceCoverage.missingEvidence, "result.evidenceCoverage.missingEvidence");

  if (!Array.isArray(result.reviewedAntiPatterns) || result.reviewedAntiPatterns.length === 0) {
    fail("result.reviewedAntiPatterns must be a non-empty array");
  }
  result.reviewedAntiPatterns.forEach((item, index) => {
    assertObject(item, `result.reviewedAntiPatterns[${index}]`);
    const allowed = new Set(["name", "present", "notes"]);
    for (const key of Object.keys(item)) {
      if (!allowed.has(key)) fail(`result.reviewedAntiPatterns[${index}].${key} is not allowed`);
    }
    assertString(item.name, `result.reviewedAntiPatterns[${index}].name`);
    assertBoolean(item.present, `result.reviewedAntiPatterns[${index}].present`);
    if (typeof item.notes !== "string") {
      fail(`result.reviewedAntiPatterns[${index}].notes must be a string`);
    }
  });

  if ("openQuestions" in result) {
    assertStringArray(result.openQuestions, "result.openQuestions");
  }
}

function buildPacket(packetName) {
  const build = spawnSync(process.execPath, ["scripts/ai-eval-packet.mjs", "build", packetName], {
    cwd: root,
    encoding: "utf8",
  });
  if (build.status !== 0) {
    process.stderr.write(build.stderr);
    process.stdout.write(build.stdout);
    fail(`failed to build packet: ${packetName}`);
  }
  const outputPath = build.stdout.trim().split("\n").at(-1);
  if (!outputPath || !existsSync(outputPath)) {
    fail(`packet builder did not produce a compiled file for ${packetName}`);
  }
  return outputPath;
}

function runEvaluator(packetName) {
  const evaluatorCommand = process.env.SEALMAIL_AI_EVALUATOR_CMD;
  if (!evaluatorCommand) {
    fail("SEALMAIL_AI_EVALUATOR_CMD is required, for example: SEALMAIL_AI_EVALUATOR_CMD='your-ai-command --json'");
  }

  const packetPath = buildPacket(packetName);
  const packet = readFileSync(packetPath, "utf8");
  const result = spawnSync(evaluatorCommand, {
    cwd: root,
    encoding: "utf8",
    input: packet,
    shell: true,
  });

  if (result.status !== 0) {
    process.stderr.write(result.stderr);
    fail(`AI evaluator command failed with exit code ${result.status}`);
  }
  const parsed = parseJson(result.stdout, "AI evaluator stdout");
  validateResult(parsed);

  mkdirSync(outDir, { recursive: true });
  const outPath = path.join(outDir, packetName.replace(/\.packet\.md$/, ".result.json"));
  writeFileSync(outPath, `${JSON.stringify(parsed, null, 2)}\n`);
  console.log(outPath);
}

function selfTest() {
  validateResult({
    scenarioId: "SELF_TEST",
    pass: false,
    score: 70,
    summary: "Self-test fixture for the AI evaluation result validator.",
    dimensionScores: {
      coreTaskClarity: { score: 4, reason: "Core task is identifiable." },
      basicReliability: { score: 4, reason: "Basic reliability evidence is present." },
      featureRestraint: { score: 3, reason: "Differentiators are reviewed with restraint." },
      stateHonesty: { score: 4, reason: "States are explicit." },
      errorActionability: { score: 3, reason: "Errors include next actions." },
      cognitiveLoad: { score: 3, reason: "The review checks for user burden." },
      evidenceSufficiency: { score: 2, reason: "The fixture intentionally has limited evidence." },
    },
    findings: [
      {
        severity: "P2",
        principle: "AI evaluator output must be machine-checkable.",
        issue: "This fixture proves the validator accepts a complete structured result.",
        evidence: "All required top-level fields, dimensions, findings, and evidence coverage are present.",
        recommendation: "Keep validating real evaluator output before storing it as regression evidence.",
      },
    ],
    evidenceCoverage: {
      factLayer: ["fixture fact evidence"],
      experienceLayer: ["fixture experience evidence"],
      missingEvidence: ["real product evidence"],
    },
    reviewedAntiPatterns: [
      {
        name: "Vague AI opinion",
        present: false,
        notes: "The fixture is structured and schema-constrained.",
      },
    ],
    openQuestions: [],
  });
  console.log("AI evaluation runner self-test OK");
}

const [command, arg] = process.argv.slice(2);

if (command === "validate-result") {
  validateResult(parseJsonFile(arg));
  console.log(`AI evaluation result OK: ${arg}`);
} else if (command === "run") {
  if (!arg) fail("usage: bun run ai-eval:run -- run <packet-file-name>");
  runEvaluator(arg);
} else if (command === "self-test") {
  selfTest();
} else {
  fail("usage: bun run ai-eval:run -- <run|validate-result|self-test> <file>");
}
