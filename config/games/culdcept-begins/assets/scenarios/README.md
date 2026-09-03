# シナリオ画像の置き場

`scenario-request.yaml` の `scenario.id` ごとにフォルダーを作り、操作順に画像を置きます。

```text
assets/scenarios/
└─ <scenario-id>/
   ├─ 00-start.jpg
   ├─ 01-screen.jpg
   ├─ 01-button.jpg       # 必要な場合のみ
   ├─ 02-screen.jpg
   └─ 99-finished.jpg
```

- 基本は、ボタンを押す直前の全画面を保存します。
- 同じ画面で押す候補が多い場合は、ボタン部分の切り抜きも保存します。
- 画像は撮影時の解像度を統一し、トリミングや拡大縮小をしない全画面版を残します。
- 現在のRust側画像デコーダーに合わせ、ひとまずJPEG (`.jpg`) を使います。
- 購入、セーブ、データ消去などの危険な画面は、YAMLの `risks` と対応stepの `notes` に明記します。

記入用YAMLは次にあります。

```text
../../authoring/scenario-request.yaml
```

画像とYAMLの記入後、それらを基に以下を作成します。

- `game.yaml`
- `scenes/*.yaml`
- シーン判定用の切り抜き画像
- 実行用シナリオYAML（実行機能の実装後）

