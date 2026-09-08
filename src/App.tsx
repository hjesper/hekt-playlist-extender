import { Fragment, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Check, ChevronRight, Disc3, Download, ExternalLink, Headphones, Library, ListMusic, Music2, Search, Settings, Sparkles, Upload, X } from "lucide-react";
import { getSummary, importPlaylist, loadTracks, previewPlaylist, resolveSeed, searchSource, setSeed } from "./api";
import type { ImportPreview, LibrarySummary, SourceSearchResult, Track } from "./types";

type Page = "library" | "import" | "discover" | "settings";
const nav = [
  { id: "library" as const, label: "Library", icon: Library },
  { id: "import" as const, label: "Import & match", icon: Upload },
  { id: "discover" as const, label: "Discover", icon: Sparkles },
  { id: "settings" as const, label: "Settings", icon: Settings },
];

export function App() {
  const [page, setPage] = useState<Page>("library");
  const [search, setSearch] = useState("");
  const summary = useQuery({ queryKey: ["summary"], queryFn: getSummary });
  const tracks = useQuery({ queryKey: ["tracks"], queryFn: loadTracks });

  return <div className="app-shell">
    <aside>
      <div className="brand"><span className="brand-mark"><Disc3 /></span><span>HEKT<small>Playlist Extender</small></span></div>
      <nav>{nav.map(({ id, label, icon: Icon }) => <button key={id} className={page === id ? "active" : ""} onClick={() => setPage(id)}><Icon size={18}/>{label}</button>)}</nav>
      <div className="sidebar-note"><span className="status-dot"/>Local library<br/><small>Your data stays on this Mac</small></div>
    </aside>
    <main>
      <header><div className="search"><Search size={17}/><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Search your library"/></div><div className="avatar">JN</div></header>
      {page === "library" && <LibraryPage summary={summary.data} tracks={(tracks.data ?? []).filter(t => `${t.artist} ${t.title}`.toLowerCase().includes(search.toLowerCase()))} onImport={() => setPage("import")}/>}
      {page === "import" && (
        <ImportPage onComplete={() => setPage("library")}/>
      )}
      {page === "discover" && (
        <DiscoverPage summary={summary.data} onReview={() => setPage("library")}/>
      )}
      {page === "settings" && <SettingsPage/>}
    </main>
    <Player/>
  </div>;
}

function LibraryPage({ summary, tracks, onImport }: { summary?: LibrarySummary; tracks: Track[]; onImport:()=>void }) {
  const client = useQueryClient();
  const [reviewing, setReviewing] = useState<number>();
  const [sourceUrls, setSourceUrls] = useState<Record<number, string>>({});
  const [sourceResults, setSourceResults] = useState<Record<number, SourceSearchResult>>({});
  const refresh = () => { client.invalidateQueries({queryKey:["tracks"]}); client.invalidateQueries({queryKey:["summary"]}); };
  const seed = useMutation({ mutationFn: ({id, selected}:{id:number;selected:boolean}) => setSeed(id, selected), onSuccess: refresh });
  const resolution = useMutation({ mutationFn: ({id,status,url}:{id:number;status:"pending"|"accepted"|"skipped";url?:string}) => resolveSeed(id,status,url), onSuccess: () => { refresh(); setReviewing(undefined); } });
  const lookup = useMutation({ mutationFn: ({track,broad=false}:{track:Track;broad?:boolean}) => searchSource(track,broad), onSuccess: (result, {track}) => setSourceResults(values => ({...values,[track.id]:result})) });
  const error = seed.error ?? resolution.error ?? lookup.error;
  return <section className="page">
    <div className="page-title"><div><p className="eyebrow">YOUR COLLECTION</p><h1>Library</h1><p>Review imported tracks and choose the strongest seeds for discovery.</p></div><button className="primary" onClick={onImport}><Upload size={17}/> Import playlist</button></div>
    <div className="stats"><Stat value={summary?.tracks ?? 0} label="Tracks in latest import"/><Stat value={summary?.selectedSeeds ?? 0} label="Selected seeds"/><Stat value={summary?.acceptedSeeds ?? 0} label="Confirmed matches" muted={!summary?.acceptedSeeds}/></div>
    {error && <div className="error"><X size={18}/>{String(error)}</div>}
    <div className="panel"><div className="panel-head"><div><h2>{summary?.importName ?? "Latest playlist"}</h2><p>{summary?.imports ? `${summary.pendingSeeds} seed match${summary.pendingSeeds === 1 ? "" : "es"} still need review` : "No playlist imported yet"}</p></div><span className="pill">{tracks.length} tracks</span></div>
      {tracks.length ? <div className="track-list"><div className="track-row labels"><span>#</span><span>Track</span><span>Details</span><span>Seed & match</span></div>{tracks.map(track => <Fragment key={track.id}><div className="track-row"><span className="number">{String(track.rowNumber).padStart(2,"0")}</span><span><b>{track.title || "Missing title"}</b><small>{track.artist || "Missing artist"}</small></span><span><small>{[track.version, track.label, track.bpm && `${track.bpm} BPM`].filter(Boolean).join(" · ") || "No extra metadata"}</small></span><span className="seed-cell"><button aria-label={`Toggle ${track.title} as seed`} className={`seed ${track.selected ? "selected" : ""}`} disabled={seed.isPending} onClick={() => seed.mutate({id:track.id,selected:!track.selected})}>{track.selected ? <Check size={15}/> : "+"}</button>{track.selected && <button className={`match-chip ${track.matchStatus ?? "pending"}`} onClick={() => setReviewing(reviewing === track.id ? undefined : track.id)}>{track.matchStatus ?? "pending"}</button>}</span></div>
        {reviewing === track.id && track.selected && <div className="match-review"><div><b>Confirm this exact recording</b><p>Search the source or paste its track page. Nothing is accepted automatically; confirm the recording and version yourself.</p></div><div className="source-search-actions"><button className="source-search-button" onClick={() => lookup.mutate({track})} disabled={lookup.isPending}>{lookup.isPending ? "Searching installed Chrome…" : "Search 1001Tracklists"}</button>{track.version && <button onClick={() => lookup.mutate({track,broad:true})} disabled={lookup.isPending}>Search without version</button>}</div>{sourceResults[track.id] && <div className="source-results">{sourceResults[track.id].tracks.length ? sourceResults[track.id].tracks.map(result => { const chosen = result.url === (sourceUrls[track.id] ?? track.sourceUrl); return <button key={result.url} className={chosen ? "selected" : ""} aria-pressed={chosen} onClick={() => setSourceUrls(values => ({...values,[track.id]:result.url}))}><span>{result.displayText}</span><small>{chosen ? <><Check size={13}/> Selected</> : "Use this track page"}</small></button>; }) : <p>No source candidates found. Try a manual URL or skip this seed.</p>}</div>}<input aria-label="1001Tracklists track URL" value={sourceUrls[track.id] ?? track.sourceUrl ?? ""} onChange={e => setSourceUrls(values => ({...values,[track.id]:e.target.value}))} placeholder="https://www.1001tracklists.com/track/…"/><div className="review-actions"><button className="primary compact" onClick={() => resolution.mutate({id:track.id,status:"accepted",url:sourceUrls[track.id] ?? track.sourceUrl})} disabled={resolution.isPending}><Check size={15}/> Confirm URL</button><button onClick={() => resolution.mutate({id:track.id,status:"skipped"})} disabled={resolution.isPending}>Skip seed</button>{track.matchStatus && track.matchStatus !== "pending" && <button onClick={() => resolution.mutate({id:track.id,status:"pending"})}>Review again</button>}{track.sourceUrl && <a href={track.sourceUrl} target="_blank" rel="noreferrer">Open source <ExternalLink size={13}/></a>}</div></div>}
      </Fragment>)}</div>
      : <Empty title="Your library is empty" body="Import a Rekordbox TXT export to start reviewing tracks." icon={<ListMusic/>} action="Import playlist" onAction={onImport}/>}</div>
  </section>;
}

function ImportPage({onComplete}:{onComplete:()=>void}) {
  const client = useQueryClient();
  const [selection, setSelection] = useState<{path:string;preview:ImportPreview}>();
  const [name, setName] = useState("");
  const choose = useMutation({ mutationFn: async () => { const path = await open({multiple:false,filters:[{name:"Rekordbox text",extensions:["txt","tsv","csv"]}]}); if (!path || typeof path !== "string") return; return {path, preview: await previewPlaylist(path)}; }, onSuccess: data => { if(data){ setSelection(data); setName(data.path.split("/").pop()?.replace(/\.[^.]+$/,"") ?? "Playlist"); } } });
  const commit = useMutation({ mutationFn: () => { if(!selection) throw new Error("Choose a playlist first"); return importPlaylist(selection.path, name.trim() || "Playlist"); }, onSuccess: () => client.invalidateQueries() });
  const error = choose.error ?? commit.error;
  return <section className="page narrow"><p className="eyebrow">NEW PLAYLIST</p><h1>Import & match</h1><p>Preview the complete export before it is written to your local library. Original rows and version details are preserved.</p>
    {!selection ? <button className="dropzone" onClick={() => choose.mutate()} disabled={choose.isPending}><span><Upload/></span><h2>{choose.isPending ? "Reading playlist…" : "Choose a Rekordbox export"}</h2><p>TXT, TSV or CSV · UTF-8 and UTF-16</p><b>Browse files <ChevronRight size={16}/></b></button>
    : <div className="import-preview panel"><div className="preview-heading"><div><span className="pill">{selection.preview.encoding} · {selection.preview.delimiter}</span><h2>{selection.preview.tracks.length} tracks detected</h2><p>{selection.preview.headers.length} columns · review the first rows before importing</p></div><button onClick={() => setSelection(undefined)}>Choose another</button></div><label>Playlist name<input value={name} onChange={e => setName(e.target.value)}/></label>{selection.preview.warnings.length > 0 && <div className="warning"><AlertTriangle size={17}/><span>{selection.preview.warnings.length} row warning{selection.preview.warnings.length === 1 ? "" : "s"}: {selection.preview.warnings.slice(0,2).join("; ")}</span></div>}<div className="preview-table">{selection.preview.tracks.slice(0,8).map(track => <div key={track.rowNumber}><span>{track.rowNumber}</span><span><b>{track.title || "Missing title"}</b><small>{track.artist || "Missing artist"}</small></span><small>{track.version ?? "Version unknown"}</small></div>)}</div>{selection.preview.tracks.length > 8 && <p className="more-rows">+ {selection.preview.tracks.length - 8} more rows</p>}<button className="primary import-button" onClick={() => commit.mutate()} disabled={commit.isPending || !selection.preview.tracks.length}>{commit.isPending ? "Importing…" : `Import ${selection.preview.tracks.length} tracks`}</button></div>}
    {error && <div className="error"><X size={18}/>{String(error)}</div>}
    {commit.data && <div className="success"><Check/><div><b>{commit.data.tracks.length} tracks imported</b><p>The playlist is ready for seed selection and match review.</p></div><button onClick={onComplete}>Review tracks</button></div>}
    <div className="info-grid"><Info n="01" title="Keep every row" text="The complete playlist is used for duplicate exclusion."/><Info n="02" title="Confirm identity" text="Ambiguous titles and versions remain available for review."/><Info n="03" title="Pick the best seeds" text="Choose up to 20 confirmed tracks for discovery."/></div>
  </section>;
}

function DiscoverPage({summary,onReview}:{summary?:LibrarySummary;onReview:()=>void}) {
  if (!summary?.selectedSeeds) return <section className="page"><Empty title="Discovery is ready for seeds" body="Import a playlist and select tracks before starting an evidence-backed discovery run." icon={<Sparkles/>} action="Review seed tracks" onAction={onReview}/></section>;
  if (summary.pendingSeeds) return <section className="page"><Empty title={`${summary.pendingSeeds} seed match${summary.pendingSeeds === 1 ? "" : "es"} need review`} body="Every selected seed must be confirmed or explicitly skipped before discovery can start." icon={<AlertTriangle/>} action="Finish match review" onAction={onReview}/></section>;
  if (!summary.acceptedSeeds) return <section className="page"><Empty title="No confirmed seeds yet" body="Every selected track was skipped. Confirm at least one exact recording before starting discovery." icon={<AlertTriangle/>} action="Review seed tracks" onAction={onReview}/></section>;
  return <section className="page"><Empty title="Seeds are ready" body={`${summary.acceptedSeeds} confirmed seed${summary.acceptedSeeds === 1 ? "" : "s"}; ${summary.skippedSeeds} skipped. Live discovery remains disabled until the source-access spike is validated.`} icon={<Sparkles/>} action="Review seed tracks" onAction={onReview}/></section>;
}

function SettingsPage(){ return <section className="page narrow"><p className="eyebrow">CONFIGURATION</p><h1>Settings</h1><div className="panel settings"><h2>Source connections</h2><Setting title="1001Tracklists adapter" text="Not configured — live access must pass the Phase 0 spike."/><Setting title="YouTube Data API" text="No key stored. Credentials will be kept in the macOS keychain."/><Setting title="Browser session" text="Installed Chrome · dedicated profile"/></div></section> }
function Setting({title,text}:{title:string;text:string}){return <div className="setting"><div><b>{title}</b><p>{text}</p></div><button disabled>Not available yet</button></div>}
function Stat({value,label,muted}:{value:string|number;label:string;muted?:boolean}){return <div className={`stat ${muted?"muted":""}`}><strong>{value}</strong><span>{label}</span></div>}
function Info({n,title,text}:{n:string;title:string;text:string}){return <div className="info"><span>{n}</span><h3>{title}</h3><p>{text}</p></div>}
function Empty({title,body,icon,action,onAction}:{title:string;body:string;icon:React.ReactNode;action:string;onAction:()=>void}){return <div className="empty"><span>{icon}</span><h2>{title}</h2><p>{body}</p><button onClick={onAction}>{action}</button></div>}
function Player(){return <footer className="player"><div className="cover"><Music2/></div><div><b>Nothing playing</b><small>Select a recommendation to audition</small></div><div className="player-line"/><button disabled><Headphones size={18}/>Player</button><button disabled><Download size={18}/>Export</button></footer>}
