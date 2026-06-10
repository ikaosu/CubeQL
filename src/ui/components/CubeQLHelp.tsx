import React from "react";

/**
 * Inline cheat-sheet for the CubeQL DSL. Toggled by the "?" button on the
 * CubeQL panel header.
 */
export const CubeQLHelp: React.FC = () => (
  <div className="cql-help">
    <h3>CubeQL — クイックリファレンス</h3>

    <p className="cql-help-lead">
      検索する手順の形式を指定するコマンド。空欄なら <code>&lt;R, U, F, D, L, B&gt;</code>(フェイスムーブのみ)。
    </p>

    <table className="cql-help-table">
      <thead>
        <tr><th>記法</th><th>意味</th></tr>
      </thead>
      <tbody>
        <tr>
          <td><code>R U R' U'</code></td>
          <td>固定リテラル列(検証用)。指定通りの順で適用。</td>
        </tr>
        <tr>
          <td><code>&lt;R, U&gt;</code></td>
          <td>R/U(と R'/R2/U'/U2)から成る任意長(0..N)の手順列。</td>
        </tr>
        <tr>
          <td><code>&lt;R, U&gt;{`{1,7}`}</code></td>
          <td>長さを 1 手〜7 手に制限。<code>{`{n}`}</code> ちょうど n、<code>{`{m,}`}</code> m 以上。</td>
        </tr>
        <tr>
          <td><code>&lt;R, U, F&gt; U</code></td>
          <td>連結。R/U/F の手順 → 最後に U。</td>
        </tr>
        <tr>
          <td><code>[F&lt;=3] &lt;R, U, F&gt;</code></td>
          <td>BaseLimit: F は合計 3 手以下。</td>
        </tr>
        <tr>
          <td><code>[F*&lt;=3]</code></td>
          <td>BaseLimit star: F / F' / F2 を合算して 3 手以下。</td>
        </tr>
        <tr>
          <td><code>[D2&lt;=0]</code></td>
          <td>D2 を禁止(他の D 系は自由)。</td>
        </tr>
      </tbody>
    </table>

    <h4>使えるムーブ</h4>
    <ul className="cql-help-moves">
      <li><b>face</b>: U D R L F B (' / 2)</li>
      <li><b>wide</b>: Uw Dw Rw Lw Fw Bw</li>
      <li><b>slice</b>: M E S</li>
      <li><b>rotation</b>: x y z(持ち替え、1 手としてカウント)</li>
    </ul>

    <h4>例</h4>
    <ul className="cql-help-examples">
      <li><code>&lt;R, U, F&gt;</code> — RUFgen の手順のみ</li>
      <li><code>&lt;U&gt; F &lt;R, U&gt; F' &lt;U&gt;</code> — AUF → F → RUgen → F' → AUF の形になっている手順</li>
      <li><code>[F2&lt;=0] &lt;R, U, F&gt;</code> — RUFgen の手順のうち、F2 を使わないもの</li>
      <li><code>&lt;R, U, F, y&gt;</code> — RUFgen で y 持ち替えを許容</li>
    </ul>

    <p className="cql-help-foot">
      文法: 大文字 / 小文字区別、空白は任意。
    </p>
  </div>
);
