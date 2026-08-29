# リアルタイム画像解析基盤

## 構成

キャプチャスレッドはUI用JPEGを送信する直前に、同じJPEGを解析ワーカーの単一スロットへ投入します。スロットに未処理フレームがある場合は新しいフレームで置き換えるため、解析が遅れてもキューとメモリは増えません。

解析ワーカーはキャプチャ、WebViewのどちらとも独立したスレッドで動作します。既定の上限は15 FPSです。#3の実機ベースライン（Capture FPS平均59.99、1フレーム16.67 ms）に対し、解析は1フレーム4 ms以内を初期予算とします。

`tauri dev`でも実機性能を確認できるよう、JPEGデコードを担う`image`、`zune-jpeg`、`zune-core`だけはdev profileで最適化します。アプリ本体は通常のdev profileのままなので、デバッグ情報と短い再ビルド時間は維持されます。

```text
Media Foundation capture (60 FPS)
  ├─ Tauri Channel → WebView preview
  └─ latest-frame slot (capacity 1, overwrite)
       └─ analysis worker (max 15 FPS)
            ├─ JPEG decode
            ├─ ROI clamp/crop
            ├─ RGB color threshold
            └─ grayscale template matching
```

## OpenCV導入方式

現在の最小例は、既にJPEGフォールバック処理で利用しているRustの`image` crate上に実装します。ROI、色しきい値、64 × 64以下のテンプレート探索だけのためにOpenCVのC++ランタイム、LLVM、DLL配布を必須化せず、標準の`cargo test`とTauriビルドを追加セットアップなしで維持するためです。

輪郭抽出、特徴量、GPU処理などOpenCV固有機能が必要になった時点では、次の方式で追加します。

- Rustバインディングは`opencv` crateを使用し、必要モジュールを`imgcodecs`と`imgproc`へ限定する
- WindowsのOpenCV 4.xは`vcpkg`のx64 dynamic tripletで固定し、DLLをTauriのbundle resourcesへ含める
- `analysis.rs`の入力・構造化結果・単一スロットは維持し、ワーカー内部だけをOpenCV実装へ差し替える
- OpenCVを追加するPRでは、同じfixtureに対する色判定とテンプレートスコアの互換テストを必須にする

`opencv` crateはシステムのOpenCVを自動同梱せず、ビルド時にOpenCVとClangの検出が必要です。このため、現段階では任意依存としてもCargoへ追加しません。

## 既定の解析

- ROI: `(x: 480, y: 270, width: 320, height: 180)`
- 有効: `true`
- 対象色: RGB `(0, 255, 0)`
- 各チャンネルの許容差: `48`
- 最大解析頻度: `15 FPS`
- テンプレート: 未設定

色判定結果にはROI平均色、一致ピクセル数、総ピクセル数、一致率が含まれます。テンプレートを設定すると、ROI内の最良位置と0〜1のスコアを返します。大きな探索範囲では比較回数を約400万回以内に抑えるため探索間隔を自動調整し、その値を`searchStep`として返します。

## Tauri API

`get_analysis_status`は設定、投入・解析・破棄・失敗フレーム数、解析FPS、平均処理時間、最新の構造化結果を返します。キャプチャ開始時に解析も開始し、停止時に解析ワーカーをjoinします。

設定例:

```ts
await invoke("configure_analysis", {
  config: {
    enabled: true,
    roi: { x: 480, y: 270, width: 320, height: 180 },
    targetColor: { red: 0, green: 255, blue: 0 },
    colorTolerance: 48,
    maxFps: 15,
  },
});
```

グレースケールテンプレート設定例:

```ts
await invoke("set_analysis_template", {
  template: {
    width: 2,
    height: 2,
    grayscale: [0, 255, 255, 0],
  },
});
```

`template: null`でテンプレート判定を無効化できます。テンプレートは1〜64ピクセル四方、`grayscale.length === width * height`で、設定中のROI内に収まる必要があります。テンプレート設定後にROIを縮小する場合も、テンプレートを収められない設定は拒否されます。

## 性能確認

画面にはCapture FPSと並べて次を表示します。

- Analysis FPS
- 平均解析時間 / キャプチャスレッドの平均投入時間
- 最新フレームのJPEGデコード / ROI色解析 / テンプレート照合時間
- ROI色一致率
- 上書き破棄フレーム数 / 投入フレーム数

解析の影響は、同じ入力で`configure_analysis`の`enabled`だけを切り替え、Capture FPS、Rendered FPS、CPU、メモリを比較します。キャプチャ側の平均投入時間にはJPEG複製と単一スロットのロック時間が含まれるため、キャプチャスレッドへ追加した直接コストも確認できます。
