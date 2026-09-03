# Automation Error の診断と途中再開

シナリオ実行ごとにAutomation Run Logを永続保存します。通常のリトライ中は診断対象にせず、シナリオが`error`で終了した時だけError Evidenceを作成します。

## 保存場所

既定ではTauriのアプリローカルデータディレクトリ内の`automation-runs`です。`SHADOWCAST_DIAGNOSTICS_ROOT`環境変数で変更できます。

各runには次を保存します。

- `manifest.json`: run、シナリオ、開始step、進行状況、終了状態
- `events.jsonl`: step、attempt、入力、シーン遷移、timeout、エラーの全イベント
- `configuration/`: 実行時のゲーム、シーン、シナリオYAML
- `screenshots/`: step開始、入力前後、シーン変化、エラーなどのJPEG
- `error-evidence.json`: そのエラーrunを指す不変の診断入口
- `resolution.json`: 確認・検証済みのScenario Repairがある場合だけ作成

ルートの`latest-error.json`は最新エラーへの案内です。`live/latest.jpg`と`live/state.json`は、キャプチャ・解析中に約1秒ごとに上書きされる現在画面とシーン判定です。厳密な対応画像は`state.json`の`imageFile`が指します。

ディレクトリ全体の上限は500 MiBです。上限へ近づいた場合だけ古いrunをrun単位で削除します。正常終了run、修復済みエラーrun、古い未解決エラーrunの順で対象にし、実行中runと最新の未解決エラーrunは保護します。保護対象だけで上限へ達した場合は、構造化イベントを優先し、新しいイベント画像を保存しません。

## Codex skill

`$game-automation-error-diagnosis`を呼び出すと、skillは実行ログ全体、エラー時画像、現在画面、実行時設定を照合します。入力候補は既存シナリオ由来か画面からの推定かを明示します。曖昧な場合は入力しません。

Recovery Trialは次の順で行います。

1. skillが1つのボタンと押下時間を提示する。
2. ユーザーがその入力を明示承認する。
3. `scripts/invoke-recovery-trial.ps1`がエラー状態、最新画面、容量、コントローラーID、Switch接続を検証する。
4. 1入力だけを送り、必ず中立化を試みる。
5. 前後の画面とシーン判定を保存し、ユーザーが想定どおり進んだか判断する。
6. 成功時は別承認後にシーン／シナリオを修復し、テスト合格後にrunを解決済みとして記録する。

## UIからの途中再開

Automation Error後もキャプチャと解析が動いていれば、現在シーンと一致するstepを再開候補として表示します。候補が1つならそのstepを表示し、複数ならドロップダウンから選択します。

`Resume from <step-id>`を押すと、バックエンドが現在シーンとstepの一致を再検証し、新しいContinuation Runを開始します。元のエラーrunは変更せず、新runの`resumedFromRunId`から追跡できます。skillやScenario Repairはautomationを自動再開しません。
