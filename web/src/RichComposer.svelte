<script lang="ts">
 // A plain-text editor that shows mentions as highlighted names and server emoji as images while the bound value
 // keeps the canonical tokens (<@id>, <:name:id>) that are sent. Chips are atomic, so the caret never lands inside one.
 import { onMount } from 'svelte';
 import { composerParts, type ComposerContext, type ComposerPart } from './lib/composer';
 export let value = '';
 export let context: ComposerContext;
 export let label = ''; export let placeholder = ''; export let disabled = false; export let boxed = false;
 export let onkeydown: ((event: KeyboardEvent) => void) | undefined = undefined;
 export let oninput: ((cursor: number) => void) | undefined = undefined;
 export let onCursor: ((cursor: number, event: Event) => void) | undefined = undefined;
 let root: HTMLDivElement, rendered: string | null = null, composing = false, range: [number, number] = [0, 0];

 $: if (root && value !== rendered) render(value);

 function chip(part: ComposerPart): Node {
  if ('url' in part) { const img = document.createElement('img'); img.className = 'composer-emoji'; img.src = part.url; img.alt = img.title = `:${part.name}:`; img.draggable = false; img.contentEditable = 'false'; img.dataset.token = part.token; return img; }
  if ('label' in part) { const span = document.createElement('span'); span.className = part.self ? 'composer-mention self' : 'composer-mention'; span.textContent = part.label; span.contentEditable = 'false'; span.dataset.token = part.token; return span; }
  return document.createTextNode(part.text);
 }
 function render(text: string) {
  const end = document.createElement('br'); end.dataset.end = '';
  root.replaceChildren(...composerParts(text, context).map(chip), end); rendered = text;
 }
 // Text of a node tree as it will be sent. The trailing <br> only keeps a final empty line visible.
 function serialize(parent: Node): string {
  let out = '';
  parent.childNodes.forEach(child => {
   if (child.nodeType === Node.TEXT_NODE) out += child.textContent ?? '';
   else if (child instanceof HTMLElement) {
    if (child.dataset.token) out += child.dataset.token;
    else if (child.tagName === 'BR') { if (!('end' in child.dataset) && child !== parent.lastChild) out += '\n'; }
    else { if (/^(DIV|P)$/.test(child.tagName) && out && !out.endsWith('\n')) out += '\n'; out += serialize(child); }
   }
  });
  return out;
 }
 function clean() {
  const nodes = [...root.childNodes];
  return nodes.at(-1) instanceof HTMLBRElement && nodes.every((node, index) => node.nodeType === Node.TEXT_NODE || node instanceof HTMLElement && (!!node.dataset.token || index === nodes.length - 1 && node.tagName === 'BR'));
 }
 function offsetAt(node: Node, offset: number) { const prefix = document.createRange(); prefix.setStart(root, 0); prefix.setEnd(node, offset); return serialize(prefix.cloneContents()).length; }
 function place(offset: number) {
  const caret = document.createRange(); let left = Math.max(0, Math.min(offset, value.length)), placed = false;
  for (const node of root.childNodes) {
   if (node.nodeType === Node.TEXT_NODE) { const length = node.textContent?.length ?? 0; if (left <= length) { caret.setStart(node, left); placed = true; break; } left -= length; }
   else if (node instanceof HTMLElement && node.dataset.token) { if (left === 0) { caret.setStartBefore(node); placed = true; break; } left -= node.dataset.token.length; if (left <= 0) { caret.setStartAfter(node); placed = true; break; } }
   else break;
  }
  if (!placed) caret.setStart(root, Math.max(0, root.childNodes.length - 1));
  caret.collapse(true); const selection = getSelection(); selection?.removeAllRanges(); selection?.addRange(caret);
  range = [offset, offset];
 }
 export function selection(): [number, number] {
  const current = getSelection();
  if (root && current?.rangeCount && root.contains(current.anchorNode)) { const r = current.getRangeAt(0); range = [offsetAt(r.startContainer, r.startOffset), offsetAt(r.endContainer, r.endOffset)]; }
  return [Math.min(range[0], value.length), Math.min(range[1], value.length)];
 }
 export function setCursor(offset: number) { root?.focus({ preventScroll: true }); if (root) place(offset); }
 export function focus() { if (!root || document.activeElement === root) return; const [, end] = selection(); root.focus({ preventScroll: true }); place(end); }
 function replace(text: string) {
  const [start, end] = selection(), caret = start + text.length;
  value = value.slice(0, start) + text + value.slice(end); render(value); place(caret); oninput?.(caret);
 }
 function input() {
  value = rendered = serialize(root);
  if (!composing && !clean()) { const [, end] = selection(); render(value); place(end); }
  oninput?.(selection()[1]);
 }
 function beforeInput(event: InputEvent) {
  if (disabled) { event.preventDefault(); return; }
  if (event.inputType === 'insertParagraph' || event.inputType === 'insertLineBreak') { event.preventDefault(); replace('\n'); }
  else if (event.inputType.startsWith('format') || event.inputType === 'insertFromDrop') event.preventDefault();
 }
 // Paste and drop take plain text only; files bubble on to the composer area, which attaches them.
 function paste(event: ClipboardEvent) { event.preventDefault(); const data = event.clipboardData; if (disabled || !data || data.files.length) return; const text = data.getData('text/plain').replace(/\r\n?/g, '\n'); if (text) replace(text); }
 function drop(event: DragEvent) { event.preventDefault(); const data = event.dataTransfer; if (disabled || !data || data.types.includes('Files')) return; const text = data.getData('text/plain').replace(/\r\n?/g, '\n'); if (text) { root.focus(); replace(text); } }
 onMount(() => {
  const track = () => { const current = getSelection(); if (current?.rangeCount && root.contains(current.anchorNode)) selection(); };
  document.addEventListener('selectionchange', track);
  return () => document.removeEventListener('selectionchange', track);
 });
</script>

<div bind:this={root} class="rich" class:boxed class:empty={value === ''} role="textbox" tabindex={disabled ? -1 : 0} aria-multiline="true" aria-label={label} aria-disabled={disabled} data-placeholder={placeholder} contenteditable={disabled ? 'false' : 'true'} spellcheck="true" {onkeydown} onbeforeinput={beforeInput} oninput={input} onpaste={paste} ondrop={drop} ondragstart={event => event.preventDefault()} oncompositionstart={() => composing = true} oncompositionend={() => { composing = false; input(); }} onclick={event => onCursor?.(selection()[1], event)} onkeyup={event => onCursor?.(selection()[1], event)}></div>

<style>
 .rich{flex:1;min-width:0;min-height:64px;max-height:160px;overflow-y:auto;padding:7px 6px;font-size:13px;line-height:1.6;white-space:pre-wrap;overflow-wrap:anywhere;outline:none;cursor:text;position:relative;text-align:left}
 .rich.empty::before{content:attr(data-placeholder);color:#70828d;position:absolute;pointer-events:none}
 .rich[aria-disabled=true]{cursor:not-allowed}
 .rich.boxed{width:100%;min-height:90px;max-height:300px;background:#111b20;border:1px solid #3a4a53;border-radius:8px;padding:11px 13px}
 .rich.boxed:focus{border-color:var(--accent);box-shadow:0 0 0 3px #a4e4c212}
 .rich :global(.composer-mention){color:#b8ead3;background:#34524a;border-radius:4px;padding:1px 4px}
 .rich :global(.composer-mention.self){color:#1b352a;background:#b3eacb}
 .rich :global(.composer-emoji){display:inline-block;width:1.6em;height:1.6em;object-fit:contain;vertical-align:middle}
</style>
