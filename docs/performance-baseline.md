# キャプチャパイプライン性能ベースライン

## 計測対象

ShadowCast → Windows Media Foundation → nokhwa → Tauri Channel → WebView2 `<img>` の経路を対象とする。通常の利用条件に合わせ、最適化済みのreleaseビルド、1280×720、60 FPS、MJPEGで計測する。

画面と`tracing`ログに表示する値の定義は次のとおり。

| 値 | 計測区間 | 備考 |
| --- | --- | --- |
| Capture FPS | `camera.frame()`完了からChannel送信成功まで | 1秒窓 |
| Rendered FPS | WebViewでJPEGの`load`が完了した回数 | 1秒窓 |
| JPEG average | Media Foundationから受け取ったJPEGバイト数 | セッション累計平均 |
| Channel | Channelへ送ったJPEGバイト数 | 1秒窓、10進Mb/s |
| Send call | `Channel::send`呼び出し時間 | 1秒窓平均。WebView側処理時間は含まない |
| Receive → draw | Channel受信からJPEGの`load`完了まで | 1秒窓平均。物理画面の走査・応答時間は含まない |
| Dropped | 受信後、より新しいフレームに置き換えられた数 | セッション累計 |

## 環境

| 項目 | 値 |
| --- | --- |
| 計測日 | 2026-08-27 |
| OS | Windows 11 Home 10.0.26200 (build 26200) |
| CPU | AMD Ryzen 7 9700X（8 cores / 16 logical processors） |
| メモリ | 32 GB |
| キャプチャデバイス | GENKI ShadowCast、USB VID `298F` / PID `1996` |
| ビルド | `npm run tauri build -- --no-bundle`（release） |
| 映像形式 | 1280×720、60 FPS要求、MJPEGパススルー |

電源設定、GPU、WebView2のバージョン、USBポート、HDMI入力内容は結果へ影響する。比較試験では同じ条件を使用し、変更した場合は結果に追記する。

## 30分連続試験

1. ShadowCastを接続し、ShadowCastを使用する他のアプリを終了する。
2. releaseビルドを起動し、`CAPTURE STATUS`のテレメトリを`ON`へ切り替える。
3. `Start capture`を押し、状態が`RUNNING`になることを確認する。
4. 別のPowerShellで次を実行する。

   ```powershell
   .\scripts\measure-performance.ps1 -DurationMinutes 30
   ```

5. `artifacts/benchmarks/`に生成されたCSVとsummary JSONを保存する。
6. 30分後も`RUNNING`であること、フレーム数が増加していること、エラーがないことを確認する。

スクリプトはTauriプロセスと、その子孫のWebView2プロセスをまとめて採取する。`cpu_percent_machine`は全論理プロセッサに対する割合、`cpu_percent_one_core`は1論理コアを100%とする合計値である。Working Setは共有ページを含むため、リーク判断にはPrivateメモリの開始・終了差も併用する。

`-IncludeUiTelemetry`を付けるとUI Automation経由で画面指標も同じCSVへ保存できる。ただしWebView2のアクセシビリティキャッシュがPrivateメモリを増加させるため、CPU／メモリの正式値にはこのオプションを使用しない。画面指標の長時間推移とリソース推移は別セッションで採取する。

### 2026-08-27 baseline

入力映像がほぼ静止した状態で30分（1802秒）計測した。全1,782サンプルが`RUNNING`で、エラー状態への遷移は0回だった。

| パイプライン指標 | 平均 | p05 | p95 | 最小 | 最大 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Capture FPS | 59.99 | 59.5 | 60.5 | 32.7 | 65.9 |
| Rendered FPS | 55.13 | 52.9 | 58.1 | 25.6 | 60.3 |
| JPEG (KiB/frame) | 18.2 | 18.2 | 18.2 | 18.2 | 18.2 |
| Channel (Mb/s) | 8.94 | 8.9 | 9.0 | 4.9 | 9.8 |
| Receive → draw (ms) | 9.24 | 8.6 | 9.9 | 7.9 | 15.3 |
| Channel send call (ms) | 0.043 | 0.017 | 0.023 | 0.016 | 14.246 |

正式計測区間では107,908フレームを転送し、8,770フレーム（8.13%）が表示前に新しいフレームへ置き換えられた。Capture FPSが55未満の窓は4回、Channel送信が0.5 msを超えた窓も4回だけで、いずれも単発の外れ値だった。一方、Rendered FPSが55未満の窓は1,027回あり、継続的な制約はWebView2側のJPEGデコード／表示である。

| リソース指標 | 平均 | p95 | 最大 | 30分増加 |
| --- | ---: | ---: | ---: | ---: |
| CPU（マシン全体比） | 3.33% | 4.49% | 6.64% | — |
| CPU（1コア=100%） | 53.27% | 71.80% | 106.22% | — |
| Working Set (MiB) | 599.98 | 616.41 | 622.71 | +25.41 |
| Private (MiB) | 671.29 | 688.66 | 696.54 | +28.04 |

リソース計測はUI Automationを無効にした別の30分（1802秒、1,783サンプル）で行った。Tauri本体と6つのWebView2子プロセスは全期間維持され、プロセス脱落はなかった。Privateメモリ増加は基準の50 MiB以内である。UI Automationを毎秒有効にした試行ではアクセシビリティキャッシュによりPrivateメモリが一時的に約160 MiB余分に増え、計測終了後に解放されたため、リソース基準から除外した。

入力から物理画面までの遅延は高速度カメラが必要なため、この環境では未測定である。ソフトウェア内で測れるWebView受信→JPEGロード完了は平均9.24 msだった。

## 入力から表示までの遅延

`Receive → draw`はWebView内だけの遅延であり、Issueで求める入力から物理画面表示までの遅延とは異なる。全経路の遅延は、同一画角に入力と表示を収めた高速度撮影で測る。

1. ボタン押下と同時に点灯するLED、または入力信号を可視化できる治具を用意する。
2. LEDとShadowCast Controllerを240 fps以上、固定露出で同時撮影する。表示側の可変リフレッシュレートは無効化する。
3. LEDが初めて点灯したフレームから、プレビューの対応画素が初めて変化したフレームまでを数える。
4. `フレーム差 ÷ 撮影FPS × 1000`をミリ秒へ換算する。
5. ウォームアップ後に30回測定し、median、p95、minimum、maximumを記録する。

ソフトウェア時計だけでは、HDMI入力、ShadowCast内部バッファ、USB転送、Media Foundation、ディスプレイ走査を共通の基準時刻で観測できない。そのため`Receive → draw`を入力遅延として扱わない。

## 画像解析の処理予算

60 FPSのフレーム間隔は16.67 msである。画像解析はキャプチャスレッドとWebView描画から分離し、キューを伸ばさず最新フレームだけを処理する。ベースライン結果から次の基準を採用する。

- 解析処理の初期上限: 1フレーム4 ms以内（1 CPUコアの約24%相当）
- キャプチャFPS: p95相当の低下後も55 FPS以上
- 描画FPS: 30分平均55 FPS以上、p05 50 FPS以上
- Channel送信呼び出し: p95 0.5 ms未満
- 追加メモリ: 30分でPrivateメモリ増加50 MiB未満
- 解析が4 msを超える場合: 全フレーム処理を行わず、最新フレーム方式で解析レートを下げる

JPEGデコードは現在WebView側だけで行う。画像解析をRust側へ追加するとMJPEGのデコード費用が新たに発生するため、最初にデコード単体を計測し、4 ms予算に収まらない場合は解析解像度の縮小、ROI限定、解析頻度の削減を優先する。

現ベースラインではCPUとChannel送信に余裕があり、4 msの解析予算を試す余地はある。ただしRendered FPSは平均55.13 FPS、p05 52.9 FPSで既に表示側の余裕が小さい。解析結果の可視化でDOM更新を毎フレーム増やさず、解析はキャプチャ／表示とは別スレッドで最新フレームだけを扱うことを開始条件とする。

## ボトルネック判定

- Capture FPSが落ち、`Send call`が増えない: Media Foundation／USB／解析処理を確認する。
- `Send call`とChannel転送量が同時に増える: IPCバックプレッシャーまたはJPEGサイズを確認する。
- Capture FPSは維持され、Rendered FPSだけ落ちる: WebViewのJPEGデコード／描画がボトルネックである。
- Privateメモリが継続的に増える: Blob URL解放、Channelキュー、画像解析バッファを確認する。
