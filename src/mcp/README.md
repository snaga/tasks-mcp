# MCP 通信 & ツールハンドラー層 (`src/mcp`)

## モジュール概要
- **モジュール名**: `mcp` (Model Context Protocol 通信 & ツールハンドラー層)
- **レイヤー**: プロトコルハンドラー層 / プレゼンテーション層

## 責務 (Responsibility)
1. **stdio JSON-RPC 2.0 通信ループ処理**:
   - MCP 2024-11-05 仕様に準拠した stdio（標準入出力）ベースの非同期行単位メッセージ送受信。
   - `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call` メッセージのルーティング。
2. **ツール定義の提供 (`tools/list`)**:
   - 4つのコアツール（`create_task_list`, `get_next_task`, `update_task_status`, `get_task_summary`）の名前、説明文、JSON Schema をクライアントへ提示。
3. **リクエスト変換 & ディスパッチ (`tools/call`)**:
   - クライアントからの引数 JSON をドメイン型（`TaskCreateInput`, `TaskStatus` 等）へバリデーション・アンパックし、`TaskManager` へディスパッチ。
4. **レスポンス変換**:
   - 正常実行結果を MCP 標準の `{ content: [{ type: "text", text: ... }], isError: false }` にラップ。
   - ドメインエラーやガードレール違反を `{ content: [{ type: "text", text: "Error: ..." }], isError: true }` に変換し、LLM が内容を解釈して次の行動を修正できるように返却。

## 依存制約 (Dependency Rules)
- **依存方向の一方通行性**:
  - 本モジュールは下位層である `src/task`（タスクマネージャー）および `src/error`（ドメインエラー型）に依存します。
  - **`src/task` から `src/mcp` への逆依存は固く禁止** されています。
- **ビジネスロジックの混入禁止**:
  - ガードレール検証、タスク状態遷移、ファイルアトミック書き込み、監査ログ追記などの業務ロジックは本レイヤーには一切含めず、すべて `src/task`（ドメイン層）に委譲します。
- **使用クレート**:
  - `tokio` (非同期 I/O), `serde_json` (JSON-RPC メッセージ処理), `tracing` (診断ログ)

## 技術選定理由 (Why)
- **stdout / stderr の完全分離**:
  - stdio を用いた MCP 通信において、標準出力（stdout）へのデバッグ出力やログの混入は JSON-RPC パースエラーを引き起こし、クライアントとの通信を即座に破断させます。
  - 本モジュールは stdout を純粋な JSON-RPC レスポンスに 100% 専有させ、すべての診断・トレース情報は stderr へ出力するアーキテクチャを担保します。
- **引き剥がしやすさ（6ヶ月テスト）の担保**:
  - プロトコル層とドメイン層を明確に分離することで、将来 MCP 以外のプロトコル（CLI 直接実行ツール、HTTP/REST API、gRPC など）を追加・差し替える場合でも、`src/task` を一切修正することなく再利用が可能です。
