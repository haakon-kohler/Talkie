// Talkie's CodeMirror facade.
//
// This is the ONLY entry point of the vendored bundle. Everything above it is
// typed Rust (see ui/src/cm.rs). Keep this surface small and stable: every
// function here is a hand-declared extern on the Rust side.
//
// Colors deliberately live in ui/styles/app.css, not here -- the highlight
// style below maps syntax tags to CSS classes and nothing else.

import { EditorState, Compartment } from "@codemirror/state";
import { EditorView, keymap, drawSelection, dropCursor, rectangularSelection } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { syntaxHighlighting, HighlightStyle, indentUnit } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { search, searchKeymap, openSearchPanel } from "@codemirror/search";

const mdHighlight = HighlightStyle.define([
  { tag: tags.heading1, class: "cm-md-h1" },
  { tag: tags.heading2, class: "cm-md-h2" },
  { tag: tags.heading3, class: "cm-md-h3" },
  { tag: [tags.heading4, tags.heading5, tags.heading6], class: "cm-md-h" },
  { tag: tags.strong, class: "cm-md-strong" },
  { tag: tags.emphasis, class: "cm-md-em" },
  { tag: tags.strikethrough, class: "cm-md-strike" },
  { tag: tags.link, class: "cm-md-link" },
  { tag: tags.url, class: "cm-md-url" },
  { tag: tags.monospace, class: "cm-md-code" },
  { tag: tags.quote, class: "cm-md-quote" },
  { tag: tags.list, class: "cm-md-list" },
  { tag: tags.contentSeparator, class: "cm-md-hr" },
  // The literal markup characters: #, *, -, `, > ...
  { tag: tags.processingInstruction, class: "cm-md-mark" },
]);

const themeCompartment = new Compartment();

function themeFor(dark) {
  return EditorView.theme({}, { dark: !!dark });
}

/**
 * Mount an editor.
 * @param {Element} parent   container element
 * @param {string} doc       initial document
 * @param {Function} onDocChanged  called (no args) on every user edit; Rust
 *                                 pulls the text via getDoc() after debouncing,
 *                                 so we never stringify the doc per keystroke.
 * @param {boolean} dark
 * @returns {EditorView} opaque handle
 */
export function init(parent, doc, onDocChanged, dark) {
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: doc ?? "",
      extensions: [
        history(),
        drawSelection(),
        dropCursor(),
        rectangularSelection(),
        EditorView.lineWrapping,
        indentUnit.of("  "),
        markdown(),
        syntaxHighlighting(mdHighlight),
        search({ top: true }),
        keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
        themeCompartment.of(themeFor(dark)),
        EditorView.updateListener.of((u) => {
          if (u.docChanged && typeof onDocChanged === "function") onDocChanged();
        }),
      ],
    }),
  });
  return view;
}

export function getDoc(view) {
  return view.state.doc.toString();
}

/** Replace the whole document, preserving scroll position where possible. */
export function setDoc(view, text) {
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
  });
}

/**
 * Insert text at `pos` and scroll it into view -- how a silent capture appears
 * in an editor that happens to be open.
 *
 * `pos` is a CodeMirror document position, i.e. a UTF-16 code unit offset, which
 * is what the Rust side converts its byte offset into before calling.
 *
 * The cursor is left where it was rather than dragged to the insertion: an
 * insertion above the caret shifts every position after it, and CodeMirror maps
 * the existing selection through the change for us. Someone mid-sentence when a
 * capture lands keeps their place.
 */
export function insertAndReveal(view, pos, text) {
  view.dispatch({
    changes: { from: pos, insert: text },
    effects: EditorView.scrollIntoView(pos, { y: "start" }),
  });
}

export function openSearch(view) {
  openSearchPanel(view);
}

export function setTheme(view, dark) {
  view.dispatch({ effects: themeCompartment.reconfigure(themeFor(dark)) });
}

export function focusEditor(view) {
  view.focus();
}

export function destroy(view) {
  view.destroy();
}
