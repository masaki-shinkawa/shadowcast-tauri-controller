# Game Automation

ゲーム画面を判定し、定義済みのシナリオに従ってコントローラー入力を進める自動操作のコンテキストです。

## Language

**Automation Error**:
シナリオが正常完了または利用者による停止ではなく、処理を継続できずに終了した状態。停止診断の対象であり、正常なリトライ中や長い待機中は含まない。
_Avoid_: Hang, Stall, Freeze

**Recoverable Automation Error**:
現在画面を取得でき、コントローラーを安全な状態から操作できるAutomation Error。Recovery Trialを提案できる。
_Avoid_: Retriable error

**Stop Diagnosis**:
Automation Error が発生した理由と、再開に必要な次の操作を判断するための調査。
_Avoid_: Hang analysis

**Error Evidence**:
Automation Run Log 内のエラー位置と関連するAutomation Snapshot、画面、入力履歴、エラー情報。Stop Diagnosis の再現可能な入口となる。
_Avoid_: Current status, Live state

**Automation Run Log**:
一回のシナリオ実行について、開始から終了までの状態、入力、判定、遷移、リトライ、エラーを時系列で保持する永続記録。画像は重要なイベント時点に関連付ける。
_Avoid_: Video recording, Debug output

**Automation Snapshot**:
同じ時点のシーン判定とシナリオ進行状況を束ねた診断用の状態記録。
_Avoid_: GameState

**Recovery Trial**:
Stop Diagnosis が推奨した1つの入力を、利用者の明示承認後に一度だけ実行して画面遷移を確かめること。入力ごとに結果を確認し、シナリオの変更そのものとは区別する。
_Avoid_: Automatic recovery, Retry

**Input Recommendation**:
Error Evidence とシナリオから導く、次に試す1つのコントローラー入力。既存シナリオ由来か画面からの推定かを区別し、候補が曖昧な場合は提示しない。
_Avoid_: Automatic input

**Scenario Repair**:
Recovery Trial の結果が想定どおりであると利用者が確認した後、その入力と遷移をシナリオへ反映すること。未登録画面を経由する場合は、その画面のシーン判定も修復対象に含む。
_Avoid_: Automatic learning

**Continuation Run**:
Automation Error 後に、現在シーンと一致する途中stepから明示的に開始する新しいシナリオ実行。元のAutomation Run Logとの関係を保持する。
_Avoid_: Retry, Automatic restart
