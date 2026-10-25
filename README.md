# CubeQL — Cube Query Language

CubeQL は **3x3x3 ルービックキューブの任意中間ステップ** (OLL / PLL / F2L / カスタムスケルトン等) を、ステッカー無視マスクと CubeQL DSL (`<R, U, F>` のような手の文法) で柔軟に表現して探索できる、汎用キューブ探索デスクトップアプリ (Windows / Tauri) です。

FMC 競技者、新規アルゴリズム探索者、メソッド研究者、教材作成者向け。

## 特徴

- **柔軟な DSL (CubeQL)** — 手の集合 `<R, U, F>` / 量化子 `<R, U>{1, 4}` / BaseLimits `[F*<=3]` / wide moves `<Rw, U>` 等を組み合わせた制約検索
- **ステッカー無視マスク** — OLL / PLL / F2L / CMLL / COLL / EOLL / OCLL プリセット + ステッカー単位カスタム
- **3 出力モード** — 最短のみ / 指定手数以下を列挙 / 上位 N 件
- **不可能性即時検出** — 部分群外の状態は探索開始前にゼロノードで返却
- **subgroup-aware PDB** — `<R, U, F>` 等の部分群制限下で tight な lower bound、CubeQL 拡張機能 (BaseLimits / quantifier) も含めて実用速度

## インストール

[Releases ページ](https://github.com/ikaosu/cubeql/releases) から以下のいずれかを取得してください:

| ファイル | 形式 | 推奨 |
|---|---|---|
| `CubeQL_X.Y.Z_x64-setup.exe` | NSIS インストーラ | 一般推奨 |
| `CubeQL_X.Y.Z_x64_en-US.msi` | MSI インストーラ | 企業環境等 |

### Windows SmartScreen 警告について

CubeQL は無署名のため、初回起動時に Windows Defender SmartScreen の警告が出ます。

「WindowsによってPCが保護されました」の画面が出たら:
1. **詳細情報** をクリック
2. **実行** ボタンをクリック

これは無償 OSS アプリ全般で出る通常の挙動で、CubeQL に害はありません。

### 動作環境

- Windows 10 / 11 (x64)
- メモリ 8GB 以上推奨 (初回起動時に subgroup-aware PDB の build で ~88MB の disk cache を作成)

## 使い方 (簡単な例)

1. アプリ起動
2. 左ペインの facelet エディタで初期状態を入力 (色を選んで各ステッカーをクリック)
3. 必要ならマスク (プリセット or 個別) を設定
4. 右ペイン上段 CubeQL に DSL を入力:
   - 例 `<R, U, F>` — R/U/F の任意の組合せで探索
   - 例 `[F*<=3] <R, U, F>` — F 系合計 3 手以内で
   - 例 `<R, U>{1,4}` — R/U 系で 1〜4 手
5. Max moves を設定、Shortest / All ≤ N / Top のいずれかを選択
6. **Start** で探索開始

詳細な DSL 文法 (literal / set / 量化子 / BaseLimits / wide / slice) は `CubeQL.md` (リポジトリ内) を参照。

## ライセンス

**GNU General Public License v3.0** — `LICENSE` 参照。

派生実装 / fork / 改変版を配布する場合も GPLv3 互換ライセンスでの公開が必要です。

CubeQL のアルゴリズム研究にあたっては Herbert Kociemba 氏の Cube Explorer (https://github.com/hkociemba/CubeExplorer , GPLv3) の Pascal source を参考にしています。

## 開発者向け: ソースから build

### 必要なツール

- Rust (cargo) 1.70+
- Node.js 20+ (npm)
- Tauri prerequisites (Windows): https://tauri.app/start/prerequisites/

### Build 手順

```powershell
# repo root の `03_develop/` 配下で:
cd 03_develop
npm install
npm run tauri -- build
```

成果物:
- `rust-core/target/release/cubeql-tauri.exe` (実行ファイル)
- `rust-core/target/release/bundle/msi/CubeQL_X.Y.Z_x64_en-US.msi`
- `rust-core/target/release/bundle/nsis/CubeQL_X.Y.Z_x64-setup.exe`

### Bench (拡張機能の性能計測)

```powershell
cd 03_develop/rust-core
cargo build --release --bin cube-cli
./target/release/cube-cli.exe bench --cases ../calibrate/bench-cases-v7.json --out ../calibrate/result.csv --repeat 3
```

## バグ報告 / 機能要望

GitHub Issues へお寄せください。再現手順 (scramble + DSL + max moves) を添えていただけると助かります。

## 既知の制約 (v0.1.1)

- 3x3x3 のみ (2x2 / 4x4 等は未対応)
- ステッカー色入力のみ (scramble 文字列直接入力は v1.1 候補)
- アニメーション再生なし (v1.1 候補)
- 解の重複表記揺れ (`R U R' U'` vs `r u r' u'`) の正規化は未実装
