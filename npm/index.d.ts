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
  /**
   * Font id from `registerFont`, or `0`/omitted for the embedded font.
   *
   * Inherited by descendant nodes, so setting it once on a card root applies
   * to every text run inside it.
   */
  font?: number;
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
  /**
   * Register font bytes, returning an id for a node's `font` field.
   *
   * Idempotent on content: the same bytes return the same id, so registering a
   * brand font once at startup is enough. `name` is used for SVG output; lookup
   * is by id. Ids are valid for the life of the process.
   */
  registerFont(name: string, bytes: Buffer | Uint8Array): number;
  /** Distinct fonts registered, including the embedded one. */
  registeredFontCount(): number;
  /** Render a node tree to PNG bytes. */
  renderPngSync(treeJson: string, width: number, height: number): Buffer;
  /** Render a node tree to an SVG string. */
  renderSvgSync(treeJson: string, width: number, height: number): string;
  /** Render a node tree to lossless WebP bytes. */
  renderWebpSync(treeJson: string, width: number, height: number): Buffer;
  /**
   * Render pages to PDF bytes with selectable text.
   *
   * No key material or environment variable is needed; every backend is
   * available under the project's MIT/Apache-2.0 licence.
   */
  renderPdfSync(
    pagesJson: string,
    width: number,
    height: number,
  ): Buffer;
}

declare const hikari: HikariNode;
export default hikari;
