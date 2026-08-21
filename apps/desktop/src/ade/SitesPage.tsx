import { ArrowClockwise, MagnifyingGlass, SquaresFour } from "@phosphor-icons/react";
import { useState } from "react";

export function SitesPage({ onCreate, onRefresh }: { onCreate?: () => void; onRefresh?: () => void }) {
  const [query, setQuery] = useState("");

  return <div className="sites-page">
    <div className="sites-toolbar" aria-label="Site actions">
      <button className="sites-refresh" type="button" onClick={onRefresh} aria-label="Refresh sites" title="Refresh sites" data-sites-action="refresh"><ArrowClockwise /></button>
      <button className="sites-create" type="button" onClick={onCreate} data-sites-action="create">Create</button>
    </div>

    <div className="sites-content">
      <header className="sites-header">
        <h1>Sites</h1>
        <p>Turn your ideas into live websites</p>
      </header>
      <label className="sites-search">
        <MagnifyingGlass />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search sites" data-sites-search />
      </label>
    </div>

    <section className="sites-empty" aria-labelledby="sites-empty-title">
      <SquaresFour />
      <h2 id="sites-empty-title">No sites yet</h2>
      <p>Build websites and apps with databases and sign-in—try “Build a team dashboard” or “Create an event signup page.”</p>
      <button type="button" onClick={onCreate} data-sites-action="create">Create new site</button>
    </section>
  </div>;
}
