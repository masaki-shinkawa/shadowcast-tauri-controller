# ShadowCast Tauri Controller

GENKI ShadowCastの映像をWindows Media Foundation経由でRustから直接取得し、Tauri 2 + ReactのUIへリアルタイム表示するMVPです。

FFmpeg、GENKI Arcade、ブラウザの`getUserMedia`は使用しません。ShadowCastがMJPEGを提供する場合、nokhwaから得た圧縮済みJPEGフレームをデコード・再エンコードせず、Tauri ChannelでWebView2へ転送します。

## 必要環境

- Windows 10 / 11
- Node.js 20以降
- Rust stable（MSVC toolchain）
- Visual Studio 2022 Build Toolsの「C++によるデスクトップ開発」
- Microsoft Edge WebView2 Runtime
- USB接続したGENKI ShadowCast

Rustが未導入の場合は[rustup](https://rustup.rs/)を導入し、次を実行してください。

```powershell
rustup default stable-msvc
```

## 起動

```powershell
npm install
npm run tauri dev
```

ShadowCastを接続した状態で`Start capture`を押します。他のカメラアプリがShadowCastを使用中の場合は閉じてください。対応フォーマットと選択結果は起動したターミナルへ`tracing`ログとして出力されます。

詳細ログが必要な場合:

```powershell
$env:RUST_LOG="shadowcast_tauri_controller=debug,nokhwa=debug"
npm run tauri dev
```

## 検証

```powershell
npm run check
npm test
npm run build
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\PerformanceMetrics.Tests.ps1
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## 性能計測

キャプチャ中は、キャプチャ／描画／解析FPS、JPEGサイズ、Channel転送量、Channel送信呼び出し時間、解析時間、WebView受信から描画までの時間、破棄フレーム数を画面と`tracing`ログへ出力します。

30分連続試験の手順、入力から実画面表示までの遅延測定方法、基準結果、画像解析へ割り当て可能な処理予算は[性能ベースライン](docs/performance-baseline.md)を参照してください。CPUとメモリのCSV採取には次のスクリプトを使用します。

詳細テレメトリは通常時の計測コストを避けるため既定で無効です。計測前に`CAPTURE STATUS`の`TELEMETRY OFF`ボタンを押して`ON`へ切り替えてください。

```powershell
.\scripts\measure-performance.ps1 -DurationMinutes 30
```

## キャプチャ方針

1. Media FoundationからWindowsのカメラ一覧を取得する
2. 名前・説明・デバイスIDに`ShadowCast`または`GENKI`を含むデバイスを選ぶ
3. 対応する解像度、FPS、フレームフォーマットをすべてログ出力する
4. MJPEGを優先し、その中で1280×720、60 FPSに最も近い形式を選ぶ
5. MJPEGなら圧縮済みバイトを直接Channelへ送る
6. MJPEGがない場合だけRGBへデコード後、JPEGへ変換して表示する

フレーム取得はUIスレッドとは別の専用スレッドで実行します。フロントエンドは描画待ちフレームをキューに積まず、画面更新ごとに最新の1フレームだけを表示します。

## 画像解析

キャプチャ開始と同時に専用の解析ワーカーを起動し、中央320 × 180のROIを最大15 FPSで解析します。解析待ちは容量1の最新フレームスロットで、遅延時は未処理フレームを上書きするため無制限に蓄積しません。既定では緑色の一致率を返し、任意のグレースケールテンプレートを設定するとROI内の最良一致位置とスコアも返します。

構造化結果、設定API、OpenCV導入方式、性能確認方法は[リアルタイム画像解析基盤](docs/image-analysis.md)を参照してください。

解析結果は `config/games/<game-id>` の YAML 定義に従ってシーンへ変換されます。初期 fixture は `loading`、`gameplay`、`result` の3シーンを持ち、未一致を `unknown` とします。連続フレーム確認、タイムアウト、ゲームID・信頼度・検出時刻・検出器別根拠を含む直近50件の遷移ログを備えています。設定形式、OCRとの比較、評価方法は[ゲームシーン判定](docs/game-state-detection.md)を参照してください。

## MVPの範囲

実装済み: デバイス検出、フォーマット列挙・選択、Start / Stop、MJPEG転送、映像表示、解像度・FPS・フォーマット・フレーム数表示、最新フレーム解析、ROI、色判定、テンプレートマッチング、tracingログ。

未実装: OpenCVランタイム、AI/OCR、実タイトル向けの校正済み設定、自動操作、コントローラー/HID/Serial制御。
