import { loadConfig as readConfig } from "./config.js";

// CLI 入口：校验配置并产生摘要。
export function runCli(raw) {
  const config = readConfig(raw);
  return summarize(config);
}

// 生成可读的命令行摘要。
function summarize(config) {
  return { count: config.length };
}
