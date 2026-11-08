# Analysis and evidence contract · 0.4.0

## Supported index

Rust and Tree-sitter index JS/TS/JSX/TSX, Python, Svelte script blocks and Go. Function declarations, lexical functions/methods, anonymous callbacks and module execution are separate nodes. Files larger than 2 MiB are skipped with a diagnostic. Gitignore and generated/dependency directory exclusions apply. Diagnostics represent parsing recovery or unsupported syntax; they are not a language compiler verdict.

`id` is a file/line/column location. `symbol_key` records lexical identity independent of line shifts. `content_hash` is a normalized-CRLF function-content hash. Duplicate lexical identities remain ambiguous. File content caching reuses only matching analyzer version and repository root, then globally rebuilds import and symbol resolution. In-memory CLI graph methods remain bounded by actual nodes, and cyclic traversal uses visited sets.

## Resolved edges

Every edge has caller, callee, source file, call-site line and resolution category. Explicit imports, namespace references and lexical scope are resolved conservatively; parameter/local shadowing blocks a package/global match. Unrelated same-name symbols in other files are never linked just because names match. Dynamic object properties, unresolved aliases, interface dispatch, indirect callbacks, runtime imports and unsupported language constructs remain unresolved.

Go handles same-package cross-file direct calls, receiver methods, and statically known typed struct fields including an enclosing method receiver inside a nested handler. Interface implementations are not guessed. Cross-package alias/type inference is incomplete. Svelte reactive/compiler transforms are not executed. TS type resolution and tsconfig aliases are not equivalent to a full compiler.

VS Code can augment a selected neighborhood using installed language providers via prepare/incoming/outgoing Call Hierarchy. The source is `language-service-static`, retains actual call sites, and remains a static candidate. Unsupported providers/timeouts retain the Rust results. Dirty documents invalidate the disk snapshot. Built-in JS/TS provider was tested locally; Go gopls and Python provider availability must be validated in the deployment environment.

## Exploration and narratives

Fuzzy search combines name, path, validated title and summary. Chinese keyword reordering has a lower-ranked character-bag fallback; it is lexical matching, not semantic embedding. HTML has bounded visible nodes with continued expansion; the CLI/MCP neighbor endpoint paginates all resolved first-order neighbors. Module links aggregate function edges and do not assert HTTP service calls.

Narrative annotations are validated separately and cannot create nodes/edges. Skill uses conversation-model, plugin uses BYOK, source-reviewed describes manual source reading. All are unconfirmed. Unchanged unique symbols can reuse descriptions; changed code expires. Fallback identifier-derived labels are explicitly uncertain, rather than claimed business analysis.

## Git and review

Local validated Git baselines are read without changing branches/worktrees or source files. Function bodies are compared with normalized newlines; incoming reach is computed independently for before and after. Removed symbols remain in the baseline graph. Edge deltas match stable identities. Nonfunction modifications are separate file-level items. Binary resources require the editor's source-control review. Renames/overloads can require manual disambiguation. Impact is potential resolved upstream reach, not causal proof of runtime behavior.

Agent claims use explicit function mentions. Ambiguous names need human disambiguation. Unmentioned callers are follow-up leads, not automatically contradictions. Checklist state is bound to the source snapshot and base commit and is invalidated on revision change.

## Memory and privacy

Only confirmed text enters native stable memory/review records. Native stable storage has a hard 1,500 cl100k-token cap; offline export has a conservative 1,500 UTF-8-byte cap. Current-source checks and external-file hashes gate stable context. Detailed records are retrieved on demand, have a 200-record cap, and support stale marking/pruning. MCP exposes read-only queries only.

Normal scanning/search/HTML viewing has no external model request. VS Code sends only selected bounded function excerpts after the user's generation action and confirmation, to the configured API. Key storage is SecretStorage scoped by origin/provider. Graph JSON, HTML, role cache, and logs do not contain API keys. Model requests are bounded, cancellable and timed out; responses cannot alter topology. No remote telemetry is emitted. Local counters cannot establish population retention or conversion rates.
