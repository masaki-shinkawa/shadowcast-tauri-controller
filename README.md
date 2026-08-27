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

キャプチャ中は、キャプチャ／描画FPS、JPEGサイズ、Channel転送量、Channel送信呼び出し時間、WebView受信から描画までの時間、破棄フレーム数を画面と`tracing`ログへ1秒ごとに出力します。

30分連続試験の手順、入力から実画面表示までの遅延測定方法、基準結果、画像解析へ割り当て可能な処理予算は[性能ベースライン](docs/performance-baseline.md)を参照してください。CPUとメモリのCSV採取には次のスクリプトを使用します。

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

## MVPの範囲

実装済み: デバイス検出、フォーマット列挙・選択、Start / Stop、MJPEG転送、映像表示、解像度・FPS・フォーマット・フレーム数表示、tracingログ。

未実装: OpenCV、AI/OCR、ゲーム状態判定、自動操作、コントローラー/HID/Serial制御。
