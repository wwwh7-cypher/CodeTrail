/**
 * AI 解析记忆 Markdown 的纯渲染与合并逻辑。
 *
 * 每次“解释此函数”成功后会向工作区的记忆文件追加一条记录；同一函数、同一内容哈希
 * 的记录会被原位替换，避免重复。这里不依赖 vscode，便于单元测试。
 */
export interface AiMemoryEntry {
  id: string;
  name: string;
  file: string;
  line: number;
  title: string;
  summary: string;
  contentHash: string;
  snapshot: string;
  project: string;
  provider: string;
  model: string;
  timestamp: string;
}

export const AI_MEMORY_HEADER = `# CodeTrail AI 解析记忆

> 本文件由 CodeTrail 在每次「解释此函数」成功后自动追加，保存 AI 对函数的职责叙述。
> 这些内容是**未确认的 AI 叙述**，不是已确认的稳定架构记忆，也不会改变调用关系。
> 可通过命令「CodeTrail: 打开 AI 解析记忆」打开；设置 \`codeTrail.aiMemory\` / \`codeTrail.aiMemoryPath\` 可关闭或改路径。
`;

const MARKER_PREFIX = '<!-- code-trail-ai:';

function oneLine(value: unknown, max: number): string {
  return String(value ?? '')
    .replace(/\r\n?/g, '\n')
    .replace(/\s*\n\s*/g, ' ')
    .replace(/<!--\s*code-trail-ai:/gi, '&lt;!-- code-trail-ai:')
    .trim()
    .slice(0, max);
}

function markerFor(id: string, hash: string): string {
  return `${MARKER_PREFIX}${oneLine(id, 500)}:${oneLine(hash, 128).replace(/[^A-Za-z0-9_.-]/g, '')} -->`;
}

function markerIdPrefix(id: string): string {
  return `${MARKER_PREFIX}${oneLine(id, 500)}:`;
}

function snapshotLabel(entry: AiMemoryEntry): string {
  const snapshot = oneLine(entry.snapshot, 16) || 'unknown';
  const project = oneLine(entry.project, 120) || '本地项目';
  return `${snapshot} · ${project}`;
}

/** 渲染一条 Markdown 记录；以 HTML 注释作为稳定标记，便于原位更新。 */
export function renderAiMemoryEntry(entry: AiMemoryEntry): string {
  const title = oneLine(entry.title, 60) || oneLine(entry.name, 60) || '未命名函数';
  const name = oneLine(entry.name, 120);
  const file = oneLine(entry.file, 400);
  const summary = oneLine(entry.summary, 400) || '（模型未给出可确认的职责描述）';
  const model = [oneLine(entry.provider, 40), oneLine(entry.model, 120)].filter(Boolean).join('/');
  return [
    markerFor(entry.id, entry.contentHash),
    `### ${title}`,
    '',
    `- 函数：\`${name}\`（\`${file}:${entry.line}\`）`,
    `- 稳定 ID：\`${oneLine(entry.id, 500)}\``,
    `- 快照：\`${snapshotLabel(entry)} · ${oneLine(entry.contentHash, 32)}\``,
    `- 模型：${model || '未知'}（BYOK）`,
    `- 时间：${oneLine(entry.timestamp, 40)}`,
    '',
    summary,
    '',
  ].join('\n');
}

/** 把条目合并进 Markdown：同 id 原位替换，其余追加；缺少标题时补上固定标题。 */
export function upsertAiMemory(text: string, entries: AiMemoryEntry[]): string {
  let document = (text || '').replace(/\r\n?/g, '\n').trim();
  if (!document) document = AI_MEMORY_HEADER.trim();
  if (!document.startsWith('# CodeTrail AI 解析记忆')) {
    document = `${AI_MEMORY_HEADER.trim()}\n\n${document}`;
  }
  let lines = document.split('\n');
  for (const entry of entries) {
    const block = renderAiMemoryEntry(entry).trim().split('\n');
    const prefix = markerIdPrefix(entry.id);
    const start = lines.findIndex(line => line.startsWith(prefix));
    if (start >= 0) {
      let end = start + 1;
      while (end < lines.length) {
        const line = lines[end];
        if (line.startsWith(MARKER_PREFIX) || line.startsWith('# ') || line.startsWith('## ')) break;
        end += 1;
      }
      while (end > start + 1 && lines[end - 1].trim() === '') end -= 1;
      lines = [...lines.slice(0, start), ...block, ...lines.slice(end)];
    } else {
      while (lines.length > 0 && lines[lines.length - 1].trim() === '') lines.pop();
      lines = [...lines, '', ...block];
    }
  }
  return `${lines.join('\n').replace(/\n{3,}/g, '\n\n').trim()}\n`;
}
