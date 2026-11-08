import { loadConfig } from "./config";
import { createApp, startServer } from "./server";

// 应用启动入口：读取配置并启动服务。
export function main() {
  const config = loadConfig("PORT=3000\nMODE=development");
  const app = createApp(config);
  return startServer(app);
}

// 重新装载配置；此入口共享相同的解析路径。
export function reloadConfig(raw: string) {
  return loadConfig(raw);
}
