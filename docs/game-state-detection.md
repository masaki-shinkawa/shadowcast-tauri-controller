# ゲームシーン判定

## 設定構成

ゲーム、シーン、操作、シナリオは役割を分離します。Issue #5 が読み込むのは `game.yaml` と `scenes/*.yaml` です。`actions/` と `scenarios/` は #6・#7 で追加します。

```text
config/games/<game-id>/
├─ game.yaml
├─ assets/
└─ scenes/
   ├─ loading.yaml
   ├─ gameplay.yaml
   └─ result.yaml
```

初期 fixture は `sample-switch-game` です。1280 × 720 のフレームを対象に、`loading`、`gameplay`、`result` の3シーンを定義しています。これは設定読込と判定を再現するための基準であり、特定タイトルでそのまま使用する設定ではありません。対象タイトルごとにディレクトリを追加し、正解ラベル付き画像で校正してください。

起動時は `sample-switch-game` を読み込みます。別ゲームへ切り替える #7 向けインターフェースは次の Tauri command です。キャプチャ中の切り替えは拒否されます。

```ts
await invoke("load_game_config", { gameId: "my-game" });
```

開発時の既定ルートは `config/games` です。`SHADOWCAST_GAME_CONFIG_ROOT` 環境変数で差し替えられ、配布時は同ディレクトリが Tauri resource として同梱されます。

## シーン定義

```yaml
id: result
priority: 300
combination: all
detectors:
  - type: template
    image: assets/result-header.pgm
    region: [600, 60, 80, 40]
    threshold: 0.92
  - type: edge_density
    region: [600, 60, 80, 40]
    difference_threshold: 80
    min_ratio: 0.01
stability:
  consecutive_frames: 3
  timeout_ms: 2000
```

`combination` は `all` または `any`、`priority` は複数シーンが一致した場合の優先順位です。同じ優先順位では信頼度、シーンIDの順で決定するため、結果は再現可能です。シーンの `stability` を省略すると `game.yaml` の既定値を使用します。

読み込み時にゲームID、重複シーンID、解像度、ROI、閾値、テンプレートの存在と配置を検証します。`unknown` は予約IDです。テンプレートのパスはゲームディレクトリ外へ出られません。

## 検出方式

| 方式 | 設定 type | 用途 | 注意点 |
| --- | --- | --- | --- |
| 輝度 | `luma` | 暗転、ロード画面 | 演出や明るさ設定の影響を受ける |
| 色比率 | `color_ratio` | 固定色のUI、ゲージ | 色補正やHDR設定ごとに校正が必要 |
| テンプレート | `template` | 固定アイコン、固定形状 | 解像度、UIスケール、言語差ごとの画像が必要 |
| 形状 | `edge_density` | 輪郭の多い固定領域 | 動画や細かい背景でも上がるため単独利用を避ける |
| OCR | 未採用 | 可変文字列、複数画面の共通ラベル | OCRランタイム、言語データ、前処理が必要 |

初期構成では高速で依存の少ない4方式を採用しました。OCRは固定ラベルに対してテンプレートより配布サイズと運用負荷が大きく、誤読文字列を信頼度へ変換する校正もタイトル・言語ごとに必要なため未採用です。文字列そのものを操作条件にするゲームでは `ocr` 検出器を追加し、同じ `DetectorEvidence` へ正規化してから利用します。

## 判定結果とログ

`get_analysis_status` の `sceneDetection` は #7 が参照する共通モデルです。

- `gameId`、`sceneId`
- 0〜1 の `confidence`
- Unix epoch ミリ秒の `detectedAtMs`
- `frameNumber`
- 検出器ごとの `evidence`（方式、一致可否、信頼度、観測値、閾値、ROI、詳細）
- `consecutiveFrames`
- 確定前候補と連続数

一致するシーンがない画面は `unknown` のままです。確定済みシーンと異なる状態が続いた場合は、設定されたタイムアウト後に `unknown` へ遷移します。候補シーンは指定フレーム数だけ連続一致するまで確定しません。映像停止や解析失敗もタイムアウト対象です。

`sceneTransitions` は直近50件を保持します。各遷移は同じ判定結果と理由を `game scene transitioned` の `tracing` イベントにも記録するため、使用した検出器まで追跡できます。

## 評価

テストは次を固定 fixture で確認します。

- 1ゲーム3シーンを YAML から読み込める
- 設定色の正例を `gameplay` として根拠付きで分類できる
- 全条件の負例を既知シーンへ分類しない
- 単発ノイズで連続確認がリセットされる
- 3フレーム連続で確定する
- フレーム停止・解析不能から2秒で `unknown` へ戻る
- パストラバーサルを含む不正ゲームIDを拒否する

```powershell
cargo test --manifest-path src-tauri/Cargo.toml game_state
```

実ゲームへ適用する際は、状態ごとの連続フレームと `unknown` を含む評価画像を用意し、シーン別の適合率・再現率と混同行列を記録してください。
