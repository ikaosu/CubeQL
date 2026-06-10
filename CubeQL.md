# CubeQL リファレンス

**CubeQL — Cube Query Language**: 3×3×3 探索ツールの move-grammar DSL。本ドキュメントは現状で受け付けている文法と意味の網羅マニュアル。

> 内部コードでは歴史的経緯から `parsePattern` / `DslError` 等の名称を維持していますが、文法そのものは **CubeQL** と総称します。

---

## 目次

1. [クイックリファレンス](#1-クイックリファレンス)
2. [基本構文](#2-基本構文)
3. [Move(手の表記)](#3-move手の表記)
4. [Move Set `<...>`](#4-move-set-)
5. [量化子 `{...}`](#5-量化子-)
6. [Base 使用制限 `[...]`](#6-base-使用制限-)
7. [評価セマンティクス](#7-評価セマンティクス)
8. [例(用途別)](#8-例用途別)
9. [エラーメッセージ](#9-エラーメッセージ)
10. [現状の制限](#10-現状の制限)

---

## 1. クイックリファレンス

| 書き方 | 意味 |
|---|---|
| `R` | R(右面 CW 1 手) |
| `R'` | R'(CCW) — Unicode prime `’ ʼ ʹ ′` も OK |
| `R2` | R2(180°) |
| `Rw'` | 二層回し(wide) Rw' |
| `M` `E` `S` | スライス move |
| `R U R' U'` | 完全固定 4 手(検証用途) |
| `<R, U>` | R/U のみで 0〜N 手 |
| `<R, U>{1,4}` | R/U のみで 1〜4 手 |
| `<R, U>{3}` | R/U のみでちょうど 3 手 |
| `<R, U>{2,}` | R/U で 2 手以上(上限はグローバル N) |
| `[F<=3] <R, U, F>` | F (CW) のみ 3 個まで(F' / F2 は自由) |
| `[F*<=3] <R, U, F>` | F 系(F/F'/F2 合算)を 3 個まで |
| `[D2<=0] <R, U, D>` | D2 のみ禁止(D, D' は自由) |
| `[F*<=3, R*<=5] R <R, U, F>{1,12} R'` | 複合 |
| `<U> F <R, U> F' <U>` | リテラルと set の混在(AUF wrap 等) |

---

## 2. 基本構文

EBNF 風表記:

```
Pattern     = [ BaseLimits ] Token { whitespace Token }
BaseLimits  = '[' BaseLimit { ',' BaseLimit } ']'
BaseLimit   = MoveBase [ "'" | "2" | "*" ] '<=' Number
Token       = LiteralMove | MoveSet [ Quantifier ]
LiteralMove = MoveBase [ Modifier ]
MoveSet     = '<' MoveBase { ',' MoveBase } '>'
Quantifier  = '{' Number '}'
            | '{' Number ',' Number '}'
            | '{' Number ',' '}'
MoveBase    = Face | Wide | Slice
Face        = 'U' | 'D' | 'L' | 'R' | 'F' | 'B'
Wide        = 'Uw' | 'Dw' | 'Lw' | 'Rw' | 'Fw' | 'Bw'
Slice       = 'M' | 'E' | 'S'
Modifier    = "'" | "2"
Number      = digit { digit }
```

- **大文字小文字を区別**します。`R` と `r` は別、現状は大文字のみが正規。
- **Token 間の区切りは空白(任意個数)**。改行も空白扱い。
- **Unicode prime** (`’` `ʼ` `ʹ` `′`) は内部で ASCII `'` に正規化されます。

### Pattern の構造

```
[ オプションの BaseLimits ] Token Token Token ...
```

例:

```
[F<=3] R U R' <R, U>{1,8} R'
└──┬──┘ └────────┬───────┘
   オプション      6 token (literal × 5 + set × 1)
```

---

## 3. Move(手の表記)

### Face turn(面 1 層)

| Base | CW | CCW | 180° |
|---|---|---|---|
| U / D | `U` `D` | `U'` `D'` | `U2` `D2` |
| R / L | `R` `L` | `R'` `L'` | `R2` `L2` |
| F / B | `F` `B` | `F'` `B'` | `F2` `B2` |

### Wide turn(2 層回し)

| Base | CW | CCW | 180° |
|---|---|---|---|
| Uw, Dw | `Uw` `Dw` | `Uw'` `Dw'` | `Uw2` `Dw2` |
| Rw, Lw | `Rw` `Lw` | `Rw'` `Lw'` | `Rw2` `Lw2` |
| Fw, Bw | `Fw` `Bw` | `Fw'` `Bw'` | `Fw2` `Bw2` |

### Slice turn(中段)

| Base | CW | CCW | 180° |
|---|---|---|---|
| M(L-R 軸) | `M` | `M'` | `M2` |
| E(U-D 軸) | `E` | `E'` | `E2` |
| S(F-B 軸) | `S` | `S'` | `S2` |

### Modifier

- `'` = CCW
- `2` = 180°
- 何もなし = CW

例: `Rw2` は「Rw 系の 180°」、`M'` は「M slice の CCW」。

---

## 4. Move Set `<...>`

カンマ区切りで base を列挙し、その base 群だけからなる **任意長の手順列** を表す。

```
<R, U>          # R, U の任意手順列(各々 CW/CCW/180° 3 variant 込み)
<R, U, F>       # R, U, F の任意手順列
<Rw, U, M>      # wide と slice も混在可能
```

### Variant 込みで展開される

`<R>` という set は **R / R' / R2 すべてを許可** します。`<R, U>` なら 2 base × 3 variant = 6 種類の手が候補。

### 同じ base を重複指定

文法上は OK(エラーにならない)が、意味的に冗長。`<R, R>` = `<R>`。

### 制限

- 0 個の base(`<>`)はエラー
- 未知の base(`<X>`)はエラー

---

## 5. 量化子 `{...}`

Move Set の直後にだけ付けられる。長さの範囲を絞る。

| 量化子 | 意味 |
|---|---|
| `{n}` | ちょうど n 手 |
| `{m,M}` | m 手 〜 M 手 |
| `{m,}` | m 手以上(上限はグローバル最大手数 N) |
| (省略) | `{0, N}`(0 手 〜 N 手) |

### 制約

- `m`, `M`, `n` は **0 以上の整数**(非負)
- `{m,M}` で `M < m` はエラー
- `M` がグローバル N より大きい場合は **N で打ち切り**

### 例

```
<R, U>            # 0..N 手
<R, U>{3}         # ちょうど 3 手
<R, U>{0,5}       # 0..5 手(空列含む)
<R, U>{1,4}       # 1..4 手(空列除く)
<R, U>{8,}        # 8 手以上、上限 N
```

---

## 6. Base 使用制限 `[...]`

**Pattern の先頭にのみ** 1 つだけ置ける、グローバル制約。pattern 全体を通しての move 使用回数を縛る。

```
[ X1<=N1, X2<=N2, ... ]
```

### Variant 接尾辞で対象を指定

| 書き方 | 対象 |
|---|---|
| `F<=3` | **F (CW) のみ** 3 個まで(F', F2 は自由) |
| `F'<=3` | **F' (CCW) のみ** 3 個まで |
| `F2<=3` | **F2 (180°) のみ** 3 個まで |
| `F*<=3` | **F 系全体**(F, F', F2 合算)3 個まで |

**重要**: 無印(`F`)は **CW のみ** を指します(対称性のため `F'` `F2` と同じく単一 variant 扱い)。3 variant 合計を縛りたい場合は **必ず `*`** を付けて `F*<=N` と書いてください。

### 組み合わせ例

| 書き方 | 意味 |
|---|---|
| `[F*<=0]` | F 系一切禁止(F, F', F2 すべて 0) |
| `[D2<=0]` | D2 のみ禁止(D, D' は自由) |
| `[D'<=0]` | D' のみ禁止(D, D2 は自由) |
| `[D<=2, D'<=2]` | D は 2 個まで、D' は 2 個まで(計 4 個まで、D2 は自由) |
| `[D*<=4, D2<=0]` | D 系全体 4 個まで **かつ** D2 個別禁止(= D + D' のみ計 4 個) |
| `[F<=3, R'<=2]` | F (CW) 3 個まで、R' (CCW) 2 個まで |

### 規則

- `<=` の左は **base 名** + 任意で `'` / `2` / `*`
- 同じ `(base, variant)` 組合せの重複指定はエラー
- `(base, null)` (= `*`、base 全体) と `(base, v)` (variant 限定) は別エントリ扱い(併用可、両方独立に enforce)
- 値は **0 以上の整数**

### 用途例

- **特定 variant の指かぶり回避**: `[D2<=0]` で D2 を避ける、`[Fw2<=0]` で Fw2 を避ける
- **base 系の完全禁止**: `[F*<=0] <R, U, F, D>` で F 系全廃
- **手数比率の制御**: `[F*<=2, R*<=4]` で軸別の最大回数を縛る
- **CW のみ縛る**: `[F<=3]`(無印が CW なので)

---

## 7. 評価セマンティクス

### グローバル最大手数 N

UI で指定。pattern 全体を展開したときの **合計手数** が **N 以下** になる解のみが対象。

- 量化子の `{m,M}` で `M > N` の場合は `M = N` に丸める
- `<...>` 単独(量化子省略) = `{0, N}` として扱われる

### 同軸枝刈り (canonical ordering)

連続して同軸 / 同 base の手を出さないように内部で枝刈り:

- **同 base の連続を禁止**(例: `<R>` 内で `R R'` は出ない — これは `identity` に等しいから)
- **同軸異 base の連続はカノニカル順序を強制**(例: `<R, L>` で `L R` は禁止、`R L` のみ。両者は順序不問だがカノニカル形に固定)

### マスク(traveling wildcard)

DSL 外で UI から指定。詳細は[マスク機能](#マスク機能):

- マスクされた sticker は「色を持たない」(wildcard)
- Move 適用時にマスクはキュービーごと一緒に動く
- 目標一致判定はマスク非対象 sticker のみで行う

### AUF(Adjustment of U Face)

専用フラグはなし。DSL で **`<U>` を前後に置く** ことで AUF 等価判定:

```
<U> R U R' U R U2 R' <U>   # Sune + AUF 自由
<U> <R, U>{1,7} <U>        # 任意 (R, U) 手順 + AUF 両端自由
```

### 各 token の独立性

連続する Move Set は **独立に展開**:

```
<R, U> <R, U>     # 前半 (R, U) と 後半 (R, U) は別個に選ぶ
                  # → <R, U>{0,2N} と同じ表現力
```

ただし境界で同軸チェックは効くので、`R U R U` のような単純連続は前半末尾の R と後半先頭の R で canonical 違反となる。

---

## 8. 例(用途別)

### OLL / PLL 探索

```
<R, U>                            # OLL を <R, U> のみで(Sune 系)
<R, U, F>{1,15}                   # OLL を <R, U, F> で 1..15 手
<U> <R, U> <U>                    # AUF 自由付き <R, U> 探索
```

### Sexy / Sledge wrap

```
<U> F <R, U> F' <U>               # F sexy wrap
<U> R U R' <U>                    # F2L 「白前白上」スロット系
```

### Conjugate / Commutator 風

```
<U, D>{1} R2 <U, D>{1}            # [U/D : R2] パターン
F <R, U>{2,10} F'                 # F-conjugate
```

### 完全固定(検証用)

```
R U R' U' R U R' U' R U R' U' R U R' U' R U R' U' R U R' U'
# Sexy×6 で identity (24 手) — round-trip 検証
```

### Wide / Slice 入り

```
<Rw, U, M>                        # Roux 風スライス含み
<R, Rw, U, F>{1,15}               # ergo な wide 含み探索
M2 U M2 U M2 <U, M2>{0,4}         # PLL H/Z 系のスライス手法
```

### Base 制限

```
[F*<=3] <R, U, F>{1,15}            # F 系合計 3 個まで(F, F', F2 計)
[F*<=0] <R, U, F, D>               # F 系完全禁止
[F<=3] <R, U, F>{1,15}             # F (CW) のみ 3 個まで(F', F2 自由)
[D2<=0] <R, U, D>{1,12}            # D2 だけ禁止
[Rw*<=2, Rw2<=0] R <R, U, F, Rw>{1,12} R'  # Rw 系合計 2 個、Rw2 禁止
[F*<=2, R*<=4] R <R, U, F>{1,12} R'  # 軸別の細かい制約
```

### F2L / CMLL 等(マスクと併用)

DSL 自体はこれらに直接対応する構文を持たず、`<R, U, F, D>` 等の **使える手集合** と **マスク** の組み合わせで表現:

```
<R, U>                            # OLL/PLL 用(マスクは UI で OLL/PLL 選択)
<R, U, F, R'>{1,18}               # F2L slot 系
<R, U, Rw>                        # CMLL 用(Roux)
```

---

## 9. エラーメッセージ

パーサが返すエラーはすべて **行・列付き**:

```
line 1, col 5: expected ">", got ","
line 1, col 12: unknown move base "X"
line 1, col 8: duplicate limit "D2"
line 1, col 15: quantifier max (3) must be >= min (5)
```

主なエラーケース:

| 状況 | メッセージ例 |
|---|---|
| 未知の base | `unknown move base "X"` |
| `<` 閉じ忘れ | `expected ">", got "..."` |
| 量化子 max < min | `quantifier max (3) must be >= min (5)` |
| BaseLimit 重複 | `duplicate limit "D2"` |
| 空の `[]` | `empty base-limits block "[]"` |
| 空の `<>` | `expected face/wide/slice identifier after "<"` |
| `<=` 抜け | `expected "<=" after base name, got "..."` |

---

## 10. 現状の制限

### 未対応の文法

| 機能 | 状況 |
|---|---|
| **持ち替え記号 `x` / `y` / `z`**(cube rotation) | **未対応**(別 round で対応予定、HTM 0 手扱い) |
| **論理 OR `A \| B`**(選言) | 未対応 |
| **ネスト set `<R, <U, F>>`** | 未対応 |
| **コメント `# ...` / `// ...`** | 未対応 |
| **変数 / マクロ `let sune = ...`** | 未対応 |
| **コミュテーター記法 `[A, B]`** | 未対応(`[`角括弧は BaseLimits で使用済み) |
| **NISS `(X)` (逆 scramble 適用)** | 未対応 |
| **繰り返し `(R U)^3`** | 未対応 |

### 探索エンジン側の特性

- **HTM(Half-Turn Metric)**: face/wide/slice の 90° / 180° 全部 1 手カウント
- **最短保証**: shortest-only / top-n / enumerate-up-to-N すべて IDDFS で **手数昇順** に解を返す
- **同軸枝刈り**: 連続同軸の冗長解は出ない(`R R'` のような無意味な並びは内部で skip)
- **マスク許容**: マスク不一致は wildcard として無視、goal は visible 部分のみで判定

### Pruning 適用条件

- `usePruningTable: true`(worker.ts デフォルト)で 4 つの admissible 下界が並列に効く
- マスクで U 面 が完全に隠れる(= F2L)場合は heuristic 無効化、mismatch ベースに fallback
- 全 corner facelet が visible(= full mask / PLL マスク等)で Corner PDB 88MB が追加適用

---

## 付録 A. 文法 ↔ 実装ファイル対応

| 機能 | ファイル |
|---|---|
| Lexer | [src/core/dsl/lexer.ts](src/core/dsl/lexer.ts) |
| Parser | [src/core/dsl/parser.ts](src/core/dsl/parser.ts) |
| Pattern IR | [src/core/dsl/pattern.ts](src/core/dsl/pattern.ts) |
| Formatter(逆引き) | [src/core/dsl/format.ts](src/core/dsl/format.ts) |
| 探索エンジン | [src/core/search/engine.ts](src/core/search/engine.ts) |
| Move 定義 | [src/core/moves.ts](src/core/moves.ts) |
| Preset(マスク) | [src/core/presets.ts](src/core/presets.ts) |

## 付録 B. 移動 base 全リスト

合計 **15 base × 3 variant = 45 種類**:

| Base index | Name | Type |
|---|---|---|
| 0 | U | Face |
| 1 | D | Face |
| 2 | R | Face |
| 3 | L | Face |
| 4 | F | Face |
| 5 | B | Face |
| 6 | Uw | Wide |
| 7 | Dw | Wide |
| 8 | Rw | Wide |
| 9 | Lw | Wide |
| 10 | Fw | Wide |
| 11 | Bw | Wide |
| 12 | M | Slice |
| 13 | E | Slice |
| 14 | S | Slice |

各 variant: 0=CW(無印)、1=CCW(`'`)、2=180°(`2`)。
