// HTML string -> satori's element-tree shape.
//
// This is the "port" BENCHMARKS.md says a fair comparison needs and does not
// have. It is small on purpose: inline `style` becomes an object, text nodes
// become strings, everything else is structural. No measurement is taken of
// this file; it exists so both renderers receive the same tree.
import { parseHTML } from 'linkedom';

export function htmlToSatori(html) {
  // Wrapped explicitly: handed a bare fragment, linkedom's document builder
  // hoists a leading <div> into <head> and leaves the body empty, which
  // silently produced an empty tree on the first attempt.
  const { document } = parseHTML(`<html><body>${html}</body></html>`);
  const el = document.body.firstElementChild;
  if (!el) throw new Error('no element to render');
  return convert(el);
}

function styleObj(el) {
  const raw = el.getAttribute('style');
  if (!raw) return undefined;
  const out = {};
  for (const decl of raw.split(';')) {
    if (!decl.trim()) continue;
    const i = decl.indexOf(':');
    if (i < 0) continue;
    const k = decl.slice(0, i).trim().replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    const v = decl.slice(i + 1).trim();
    if (k) out[k] = v;
  }
  return Object.keys(out).length ? out : undefined;
}

function convert(el) {
  if (!el || el.nodeType !== 1) return null;
  const children = [];
  for (const n of el.childNodes) {
    if (n.nodeType === 3) {
      const t = n.textContent;
      if (t && t.trim()) children.push(t);
    } else if (n.nodeType === 1) {
      const c = convert(n);
      if (c) children.push(c);
    }
  }
  const style = styleObj(el);
  const props = { children: children.length ? children : undefined };
  if (style) props.style = style;
  return { type: el.tagName.toLowerCase(), props };
}
