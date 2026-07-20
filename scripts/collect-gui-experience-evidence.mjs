#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const outDir = path.join(root, "tmp", "ai-evaluation");

const scenarioChecks = [
  {
    scenarioId: "S02",
    name: "daily inbox",
    sourceFiles: ["src/components/Sidebar.tsx", "src/components/MailList.tsx"],
    requiredMarkers: [
      ["src/components/Sidebar.tsx", "邮箱"],
      ["src/components/Sidebar.tsx", "已连接账户"],
      ["src/components/MailList.tsx", "显示"],
      ["src/components/MailList.tsx", "缓存"],
      ["src/components/MailList.tsx", "刷新"],
      ["src/components/MailList.tsx", "全部"],
      ["src/components/MailList.tsx", "未读"],
      ["src/components/MailList.tsx", "★ 星标"],
      ["src/components/MailList.tsx", "正在读取本地缓存…"],
      ["src/components/MailList.tsx", "此目录暂无邮件"],
      ["src/components/MailList.tsx", "重试"],
      ["src/components/MailList.tsx", "加载更早的邮件"],
    ],
  },
  {
    scenarioId: "S03",
    name: "read mail",
    sourceFiles: ["src/components/MessageView.tsx"],
    requiredMarkers: [
      ["src/components/MessageView.tsx", "msg-subject"],
      ["src/components/MessageView.tsx", "msg-fromname"],
      ["src/components/MessageView.tsx", "msg-addr"],
      ["src/components/MessageView.tsx", "查看验证详情"],
      ["src/components/MessageView.tsx", "回复"],
      ["src/components/MessageView.tsx", "回复全部"],
      ["src/components/MessageView.tsx", "转发"],
      ["src/components/MessageView.tsx", "收件人"],
      ["src/components/MessageView.tsx", "抄送"],
      ["src/components/MessageView.tsx", "保存"],
    ],
  },
  {
    scenarioId: "S04",
    name: "send mail",
    sourceFiles: ["src/components/ComposeModal.tsx"],
    requiredMarkers: [
      ["src/components/ComposeModal.tsx", "写邮件 · 撰写"],
      ["src/components/ComposeModal.tsx", "发自"],
      ["src/components/ComposeModal.tsx", "收件人地址"],
      ["src/components/ComposeModal.tsx", "抄送"],
      ["src/components/ComposeModal.tsx", "主题"],
      ["src/components/ComposeModal.tsx", "正文…"],
      ["src/components/ComposeModal.tsx", "添加附件"],
      ["src/components/ComposeModal.tsx", "撤销发送"],
      ["src/components/ComposeModal.tsx", "等待发送"],
      ["src/components/ComposeModal.tsx", "已签名并发送"],
      ["src/components/ComposeModal.tsx", "已发送（未签名）"],
    ],
  },
  {
    scenarioId: "S05",
    name: "draft recovery",
    sourceFiles: ["src/components/ComposeModal.tsx", "src/components/DraftsPane.tsx"],
    requiredMarkers: [
      ["src/components/ComposeModal.tsx", "草稿自动保存"],
      ["src/components/ComposeModal.tsx", "关闭前最后存一次"],
      ["src/components/ComposeModal.tsx", "草稿保存失败"],
      ["src/components/DraftsPane.tsx", "草稿"],
      ["src/components/DraftsPane.tsx", "没有草稿。写信时会自动保存，关掉也不丢。"],
      ["src/components/DraftsPane.tsx", "删除草稿"],
      ["src/components/DraftsPane.tsx", "（未填收件人）"],
      ["src/components/DraftsPane.tsx", "（无主题）"],
    ],
  },
  {
    scenarioId: "S06",
    name: "attachment handling",
    sourceFiles: ["src/components/ComposeModal.tsx", "src/components/MessageView.tsx"],
    requiredMarkers: [
      ["src/components/ComposeModal.tsx", "选择附件"],
      ["src/components/ComposeModal.tsx", "添加附件"],
      ["src/components/ComposeModal.tsx", "attach-chip"],
      ["src/components/MessageView.tsx", "保存附件"],
      ["src/components/MessageView.tsx", "保存中…"],
      ["src/components/MessageView.tsx", "已保存 ✓"],
      ["src/components/MessageView.tsx", "失败："],
      ["src/components/MessageView.tsx", "attach-save"],
    ],
  },
  {
    scenarioId: "S07",
    name: "organize mail",
    sourceFiles: ["src/components/Sidebar.tsx", "src/components/MailList.tsx", "src/components/MessageView.tsx"],
    requiredMarkers: [
      ["src/components/Sidebar.tsx", "新建目录"],
      ["src/components/Sidebar.tsx", "删除目录"],
      ["src/components/Sidebar.tsx", "过滤规则"],
      ["src/components/MailList.tsx", "全部标为已读"],
      ["src/components/MailList.tsx", "加星标"],
      ["src/components/MessageView.tsx", "移动到…"],
      ["src/components/MessageView.tsx", "归档"],
      ["src/components/MessageView.tsx", "标为未读"],
      ["src/components/MessageView.tsx", "屏蔽发件人"],
      ["src/components/MessageView.tsx", "删除"],
    ],
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

function lineOf(source, marker) {
  const index = source.indexOf(marker);
  if (index < 0) return null;
  return source.slice(0, index).split("\n").length;
}

function collect() {
  const sourceCache = new Map();
  const readSource = (file) => {
    if (!sourceCache.has(file)) {
      sourceCache.set(file, readProjectFile(file));
    }
    return sourceCache.get(file);
  };

  const scenarios = scenarioChecks.map((scenario) => {
    const markers = scenario.requiredMarkers.map(([file, marker]) => {
      const source = readSource(file);
      const line = lineOf(source, marker);
      if (line === null) {
        fail(`${file} missing GUI experience marker "${marker}" for ${scenario.scenarioId} ${scenario.name}`);
      }
      return { file, marker, present: line !== null, line };
    });
    return {
      scenarioId: scenario.scenarioId,
      name: scenario.name,
      sourceFiles: scenario.sourceFiles,
      markers,
    };
  });

  if (process.exitCode) {
    process.exit(process.exitCode);
  }

  const evidence = {
    generatedAt: new Date().toISOString(),
    kind: "static-gui-source-evidence",
    limits: [
      "This proves expected GUI controls, labels, and states exist in source code.",
      "It does not replace runtime screenshots, DOM snapshots, or manual product review.",
      "Runtime L5 evidence should be added by Tauri/browser automation in a later layer.",
    ],
    scenarios,
  };

  mkdirSync(outDir, { recursive: true });
  const outPath = path.join(outDir, "experience-evidence.json");
  writeFileSync(outPath, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(outPath);
}

collect();
