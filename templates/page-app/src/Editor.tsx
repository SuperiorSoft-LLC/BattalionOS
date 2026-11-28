// SPDX-License-Identifier: Apache-2.0
// The open text file: a text area and Save. It only shows; App owns the file and the save.
export type Doc = { state: "none" } | { state: "loading"; path: string } | { state: "error"; path: string; message: string } | { state: "ready"; path: string; text: string; dirty: boolean; saving: boolean };

interface Props {
  doc: Doc;
  onChange(text: string): void;
  onSave(): void;
}

export function Editor({ doc, onChange, onSave }: Props) {
  if (doc.state === "none") return <section className="editor muted">Choose a file to edit it.</section>;
  if (doc.state === "loading") return <section className="editor" role="status">Opening {doc.path}…</section>;
  if (doc.state === "error")
    return (
      <section className="editor" role="alert">
        <h2>{doc.path}</h2>
        <p className="error">{doc.message}</p>
      </section>
    );
  return (
    <section className="editor">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          onSave();
        }}
      >
        <div className="bar">
          <label htmlFor="text">{doc.path}{doc.dirty ? " •" : ""}</label>
          <button type="submit" disabled={!doc.dirty || doc.saving}>{doc.saving ? "Saving…" : "Save"}</button>
        </div>
        <textarea id="text" value={doc.text} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
      </form>
    </section>
  );
}
