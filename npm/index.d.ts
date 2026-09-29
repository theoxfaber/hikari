/**
 * Type declarations for `@hikari-rs/node`.
 *
 * The bindings take the node tree as a JSON string, deserialised straight into
 * the Rust `Node` type with serde. Two consequences worth knowing:
 *
 * 1. `Node` is **externally tagged** (no `#[serde(tag = ...)]`), so a node is
 *    `{"Text": { ... }}`, not `{ "type": "text", ... }`.
 * 2. Field names are the Rust names in `snake_case`, and every field of
 *    `Style` is optional, so partial styles are fine.
 */

/// Solid colour. Accepts a CSS string (`"#rrggbb"`, `"#rgb"`, `"#rrggbbaa"`)
/// or an `{r,g,b,a}` object.
export type Rgba = { r: number; g: number; b: number; a?: number } | string;

export type Background = { Solid: Rgba } | Rgba | string;

export type ImgFit = 'Cover' | 'Contain' | 'Fill';

export type Display = 'Flex' | 'Grid' | 'Block';
export type Dir = 'Row' | 'Column';
export type Justify =
  | 'Start'
  | 'Center'
  | 'End'
  | 'SpaceBetween'
  | 'SpaceAround'
  | 'SpaceEvenly';
export type Align = 'Start' | 'Center' | 'End' | 'Stretch';

/// Every field is optional; omitted fields fall back to the Rust default.
export interface Style {
  width?: number | null;
  height?: number | null;
  display?: Display;
  dir?: Dir;
  justify?: Justify;
  align?: Align;
  gap?: number;
  padding?: number;
  margin?: number;
  border?: number;
  border_color?: Rgba | null;
  absolute?: boolean;
  left?: number | null;
  top?: number | null;
  background?: Background | null;
  /**
   * Text colour. A CSS string (`"#ffffff"`) or an `{r,g,b,a}` object.
   */
  color?: Rgba | null;
  font_size?: number | null;
  radius?: number;
  grow?: number;
  grid_cols?: number | null;
  max_width?: number | null;
  aspect?: number | null;
}

export type Node =
  | { Container: { style?: Style; children?: Node[] } }
  | { Text: { text: string; style?: Style } }
  | { Image: { bytes: number[]; fit?: ImgFit; style?: Style } };

export interface HikariNode {
  /** Semver string of the native binding. */
  version(): string;
  /** Render a node tree to PNG bytes. */
  renderPngSync(treeJson: string, width: number, height: number): Buffer;
  /** Render a node tree to an SVG string. */
  renderSvgSync(treeJson: string, width: number, height: number): string;
  /** Render a node tree to lossless WebP bytes. */
  renderWebpSync(treeJson: string, width: number, height: number): Buffer;
  /**
   * Render pages to PDF bytes with selectable text.
   *
   * Pro feature. Requires `HIKARI_LICENSE` (an `hk1.…` key) and
   * `HIKARI_PUBKEY` (64 hex chars, the 32-byte ed25519 verify key) in the
   * environment. Throws if either is missing or rejected.
   */
  renderPdfSync(
    pagesJson: string,
    width: number,
    height: number,
  ): Buffer;
}

declare const hikari: HikariNode;
export default hikari;
