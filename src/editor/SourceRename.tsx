import { useState } from "react";
import type { Asset } from "./project";

export function SourceRename({ asset, busy, save, close }: {
  asset: Asset; busy: boolean; save: (id: string, name: string) => Promise<boolean>; close: () => void;
}) {
  const [name, setName] = useState(asset.name);
  return <form className="source-rename" onSubmit={async (event) => {
    event.preventDefault();
    if (name.trim() && await save(asset.id, name.trim())) close();
  }}>
    <input aria-label="새 소스 이름" autoFocus value={name} onFocus={(event) => event.currentTarget.select()}
      onKeyDown={(event) => { if (event.key === "Escape") close(); }} onChange={(event) => setName(event.target.value)} />
    <div><button className="button compact" disabled={busy || !name.trim()}>저장</button>
      <button type="button" className="button compact" onClick={close}>취소</button></div>
  </form>;
}
