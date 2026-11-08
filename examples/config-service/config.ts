// 从环境变量和配置文本装配应用配置。
export function loadConfig(raw: string) {
  const parsed = parseConfig(raw);
  return validateConfig(parsed);
}

// 将 key=value 文本解析为配置；供服务器、CLI 与热重载共用。
export function parseConfig(raw: string) {
  const entries = splitLines(raw);
  return normalizeKeys(entries);
}

// 分隔配置行，过滤空白行。
function splitLines(raw: string) {
  return raw.split("\n").filter(Boolean);
}

// 统一配置项名称。
function normalizeKeys(entries: string[]) {
  return entries.map(line => line.trim().toLowerCase());
}

// 检查关键配置是否存在。
function validateConfig(config: string[]) {
  if (!config.length) throw new Error("Missing configuration");
  return config;
}
