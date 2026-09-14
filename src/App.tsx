import { Fragment, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Check, ChevronRight, Disc3, Download, ExternalLink, Headphones, Heart, Library, ListMusic, Music2, Pause, Play, RotateCcw, Search, Settings, Sparkles, Upload, X } from "lucide-react";
import { attachAudioSource, controlDiscovery, executeDiscovery, exportShortlist, getLatestDiscoveryRun, getSummary, importPlaylist, listRecommendations, loadTracks, markAudioSourceWrongVersion, openDiscoveryBrowser, previewPlaylist, resolveSeed, searchSource, setRecommendationFeedback, setSeed, startDiscovery, verifySourceTrack } from "./api";
import type { DiscoveryRun, ImportPreview, LibrarySummary, Recommendation, SourceSearchResult, SourceTrackDetail, Track } from "./types";

type Page = "library" | "import" | "discover" | "results" | "settings";
const nav = [
  { id: "import" as const, label: "Import & match", icon: Upload },
  { id: "library" as const, label: "Library", icon: Library },
  { id: "discover" as const, label: "Discover", icon: Sparkles },
  { id: "results" as const, label: "Results", icon: Headphones },
  { id: "settings" as const, label: "Settings", icon: Settings },
];

export function App() {
  const [page, setPage] = useState<Page>("library");
  const [search, setSearch] = useState("");
  const [playing,setPlaying]=useState<Recommendation>();
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
      {page === "results" && <ResultsPage onPlay={setPlaying}/>}
      {page === "settings" && <SettingsPage/>}
    </main>
    <Player recommendation={playing} onExport={exportShortlist}/>
  </div>;
}

function LibraryPage({ summary, tracks, onImport }: { summary?: LibrarySummary; tracks: Track[]; onImport:()=>void }) {
  const client = useQueryClient();
  const [reviewing, setReviewing] = useState<number>();
  const [sourceUrls, setSourceUrls] = useState<Record<number, string>>({});
  const [sourceResults, setSourceResults] = useState<Record<number, SourceSearchResult>>({});
  const [verifiedTracks, setVerifiedTracks] = useState<Record<number, SourceTrackDetail>>({});
  const refresh = () => { client.invalidateQueries({queryKey:["tracks"]}); client.invalidateQueries({queryKey:["summary"]}); };
  const seed = useMutation({ mutationFn: ({id, selected}:{id:number;selected:boolean}) => setSeed(id, selected), onSuccess: refresh });
  const resolution = useMutation({ mutationFn: ({id,status,url}:{id:number;status:"pending"|"accepted"|"skipped";url?:string}) => resolveSeed(id,status,url), onSuccess: () => { refresh(); setReviewing(undefined); } });
  const lookup = useMutation({ mutationFn: ({track,broad=false}:{track:Track;broad?:boolean}) => searchSource(track,broad), onSuccess: (result, {track}) => setSourceResults(values => ({...values,[track.id]:result})) });
  const verification = useMutation({ mutationFn: ({id,url}:{id:number;url:string}) => verifySourceTrack(url), onSuccess: (result, {id}) => setVerifiedTracks(values => ({...values,[id]:result})) });
  const error = seed.error ?? resolution.error ?? lookup.error ?? verification.error;
  return <section className="page">
    <div className="page-title"><div><p className="eyebrow">YOUR COLLECTION</p><h1>Library</h1><p>Review imported tracks and choose the strongest seeds for discovery.</p></div><button className="primary" onClick={onImport}><Upload size={17}/> Import playlist</button></div>
    <div className="stats"><Stat value={summary?.tracks ?? 0} label="Tracks in latest import"/><Stat value={summary?.selectedSeeds ?? 0} label="Selected seeds"/><Stat value={summary?.acceptedSeeds ?? 0} label="Confirmed matches" muted={!summary?.acceptedSeeds}/></div>
    {error && <div className="error"><X size={18}/>{String(error)}</div>}
    <div className="panel"><div className="panel-head"><div><h2>{summary?.importName ?? "Latest playlist"}</h2><p>{summary?.imports ? `${summary.pendingSeeds} seed match${summary.pendingSeeds === 1 ? "" : "es"} still need review` : "No playlist imported yet"}</p></div><span className="pill">{tracks.length} tracks</span></div>
      {tracks.length ? <div className="track-list"><div className="track-row labels"><span>#</span><span>Track</span><span>Details</span><span>Seed & match</span></div>{tracks.map(track => <Fragment key={track.id}><div className="track-row"><span className="number">{String(track.rowNumber).padStart(2,"0")}</span><span><b>{track.title || "Missing title"}</b><small>{track.artist || "Missing artist"}</small></span><span><small>{[track.version, track.label, track.bpm && `${track.bpm} BPM`].filter(Boolean).join(" · ") || "No extra metadata"}</small></span><span className="seed-cell"><button aria-label={`Toggle ${track.title} as seed`} className={`seed ${track.selected ? "selected" : ""}`} disabled={seed.isPending} onClick={() => seed.mutate({id:track.id,selected:!track.selected})}>{track.selected ? <Check size={15}/> : "+"}</button>{track.selected && <button className={`match-chip ${track.matchStatus ?? "pending"}`} onClick={() => setReviewing(reviewing === track.id ? undefined : track.id)}>{track.matchStatus ?? "pending"}</button>}</span></div>
        {reviewing === track.id && track.selected && <div className="match-review">
          <div><b>Confirm this exact recording</b><p>Search the source or paste its track page. Nothing is accepted automatically; confirm the recording and version yourself.</p></div>
          <div className="source-search-actions"><button className="source-search-button" onClick={() => lookup.mutate({track})} disabled={lookup.isPending || verification.isPending}>{lookup.isPending ? "Searching installed Chrome…" : "Search 1001Tracklists"}</button>{track.version && <button onClick={() => lookup.mutate({track,broad:true})} disabled={lookup.isPending || verification.isPending}>Search without version</button>}</div>
          {sourceResults[track.id] && <div className="source-results">{sourceResults[track.id].tracks.length ? sourceResults[track.id].tracks.map(result => { const chosen = result.url === (sourceUrls[track.id] ?? track.sourceUrl); return <button key={result.url} className={chosen ? "selected" : ""} aria-pressed={chosen} onClick={() => setSourceUrls(values => ({...values,[track.id]:result.url}))}><span>{result.displayText}</span><small>{chosen ? <><Check size={13}/> Selected</> : "Use this track page"}</small></button>; }) : <p>No source candidates found. Try a manual URL or skip this seed.</p>}</div>}
          <input aria-label="1001Tracklists track URL" value={sourceUrls[track.id] ?? track.sourceUrl ?? ""} onChange={e => setSourceUrls(values => ({...values,[track.id]:e.target.value}))} placeholder="https://www.1001tracklists.com/track/…"/>
          {verifiedTracks[track.id] && <div className="access-verified"><Check size={16}/><span><b>Source access verified</b><small>{verifiedTracks[track.id].title} · {verifiedTracks[track.id].appearances.length} appearance{verifiedTracks[track.id].appearances.length === 1 ? "" : "s"} found on this page</small></span></div>}
          {verification.isPending && <div className="browser-wait"><span className="status-dot"/><span><b>Waiting for the dedicated Chrome window</b><small>If 1001Tracklists asks for attention, complete it there. Hekt will resume automatically.</small></span></div>}
          <div className="review-actions">
            <button className="primary compact" onClick={() => resolution.mutate({id:track.id,status:"accepted",url:sourceUrls[track.id] ?? track.sourceUrl})} disabled={resolution.isPending || verification.isPending}><Check size={15}/> Confirm URL</button>
            <button onClick={() => { const url = sourceUrls[track.id] ?? track.sourceUrl; if (url) verification.mutate({id:track.id,url}); }} disabled={verification.isPending || !(sourceUrls[track.id] ?? track.sourceUrl)}>{verification.isPending ? "Waiting for Chrome…" : "Open browser & verify access"}</button>
            <button onClick={() => resolution.mutate({id:track.id,status:"skipped"})} disabled={resolution.isPending || verification.isPending}>Skip seed</button>
            {track.matchStatus && track.matchStatus !== "pending" && <button onClick={() => resolution.mutate({id:track.id,status:"pending"})}>Review again</button>}
            {track.sourceUrl && <a href={track.sourceUrl} target="_blank" rel="noreferrer">Open source <ExternalLink size={13}/></a>}
          </div>
        </div>}
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
  const client = useQueryClient();
  const run = useQuery({
    queryKey: ["discovery-run"],
    queryFn: getLatestDiscoveryRun,
    refetchInterval: query => query.state.data?.status === "running" ? 1_000 : false,
  });
  const start = useMutation({mutationFn:startDiscovery,onSuccess:data => client.setQueryData(["discovery-run"],data)});
  const control = useMutation({mutationFn:({runId,action}:{runId:number;action:"pause"|"resume"|"cancel"}) => controlDiscovery(runId,action),onSuccess:data => client.setQueryData(["discovery-run"],data)});
  const execute = useMutation({
    mutationFn: executeDiscovery,
    onMutate: runId => client.setQueryData<DiscoveryRun | null>(["discovery-run"], current => current?.id === runId ? {...current,status:"running"} : current),
    onSuccess: data => {
      client.setQueryData(["discovery-run"],data);
      client.invalidateQueries({queryKey:["recommendations"]});
    },
    onError: () => client.invalidateQueries({queryKey:["discovery-run"]}),
  });
  const browser = useMutation({
    mutationFn: openDiscoveryBrowser,
    onSuccess: data => client.setQueryData(["discovery-run"], data),
    onError: () => client.invalidateQueries({queryKey:["discovery-run"]}),
  });
  const error = run.error ?? start.error ?? control.error ?? execute.error ?? browser.error;
  const canStart = !run.data || ["cancelled","completed","completed_with_errors","failed"].includes(run.data.status);
  if (!summary?.selectedSeeds) return <section className="page"><Empty title="Discovery is ready for seeds" body="Import a playlist and select tracks before starting an evidence-backed discovery run." icon={<Sparkles/>} action="Review seed tracks" onAction={onReview}/></section>;
  if (summary.pendingSeeds) return <section className="page"><Empty title={`${summary.pendingSeeds} seed match${summary.pendingSeeds === 1 ? "" : "es"} need review`} body="Every selected seed must be confirmed or explicitly skipped before discovery can start." icon={<AlertTriangle/>} action="Finish match review" onAction={onReview}/></section>;
  if (!summary.acceptedSeeds) return <section className="page"><Empty title="No confirmed seeds yet" body="Every selected track was skipped. Confirm at least one exact recording before starting discovery." icon={<AlertTriangle/>} action="Review seed tracks" onAction={onReview}/></section>;
  if (run.isLoading) return <section className="page"><Empty title="Loading discovery" body="Reading persisted run state from the local library." icon={<Sparkles/>} action="Review seed tracks" onAction={onReview}/></section>;
  return <section className="page">
    <div className="page-title"><div><p className="eyebrow">BOUNDED DISCOVERY</p><h1>Discover</h1><p>Prepare a durable, resumable source queue from your confirmed seeds.</p></div>{canStart && <div className="page-actions"><button className="primary" onClick={() => start.mutate(false)} disabled={start.isPending}><Play size={17}/>{start.isPending ? "Preparing…" : run.data ? "Prepare another run" : "Prepare discovery run"}</button>{run.data && <button onClick={() => start.mutate(true)} disabled={start.isPending}>Prepare fresh source run</button>}</div>}</div>
    {error && <div className="error"><X size={18}/>{String(error)}</div>}
    {!run.data ? <div className="discovery-ready panel"><div><span className="pill">Ready to discover</span><h2>{summary.acceptedSeeds} confirmed seed{summary.acceptedSeeds === 1 ? "" : "s"} ready</h2><p>The durable queue resumes safely, retains partial results, and ranks every identified candidate after bounded extraction.</p></div><div className="budget-grid"><Budget value="25" label="appearances per seed"/><Budget value="100" label="unique tracklists"/><Budget value="1" label="active page"/></div><div className="warning"><AlertTriangle size={17}/><span>If the source presents a challenge, Hekt pauses for normal interaction in its dedicated Chrome profile. It never solves challenges automatically.</span></div></div>
    : <DiscoveryRunCard
        run={run.data}
        busy={control.isPending}
        onControl={action => control.mutate({runId:run.data!.id,action})}
        onExecute={()=>execute.mutate(run.data!.id)} executing={execute.isPending}
        onOpenBrowser={()=>browser.mutate(run.data!.id)} openingBrowser={browser.isPending}
      />}
  </section>;
}

function DiscoveryRunCard({run,busy,onControl,onExecute,executing,onOpenBrowser,openingBrowser}:{run:DiscoveryRun;busy:boolean;onControl:(action:"pause"|"resume"|"cancel")=>void;onExecute:()=>void;executing:boolean;onOpenBrowser:()=>void;openingBrowser:boolean}) {
  const finished = run.completedJobs + run.failedJobs;
  const progress = run.totalJobs ? Math.round((finished / run.totalJobs) * 100) : 0;
  const canExecute = ["queued","paused","waiting_for_browser"].includes(run.status);
  const active = ["queued","running","waiting_for_browser","paused"].includes(run.status);
  const waitingForBrowser = run.status === "waiting_for_browser";
  return <div className="run-card panel"><div className="run-heading"><div><span className={`run-status ${run.status}`}>{run.status.replaceAll("_"," ")}</span><h2>Discovery run #{run.id}</h2><p>{run.message}</p></div><div className="run-actions">{waitingForBrowser&&<button className="primary compact" onClick={onOpenBrowser} disabled={openingBrowser||executing}>{openingBrowser?<><span className="status-dot"/>Waiting for Chrome…</>:<><ExternalLink size={15}/>Open Chrome once</>}</button>}{canExecute&&<button className={waitingForBrowser?"":"primary compact"} onClick={onExecute} disabled={executing||openingBrowser}>{executing?<><span className="status-dot"/>Working headlessly…</>:<><Play size={15}/>{waitingForBrowser?"Retry headlessly":run.status === "queued"?"Run headlessly":"Resume headlessly"}</>}</button>}{["queued","running","waiting_for_browser"].includes(run.status)&&<button onClick={() => onControl("pause")} disabled={busy}><Pause size={15}/>Pause</button>}{active&&<button onClick={() => onControl("cancel")} disabled={busy}>Cancel</button>}</div></div><div className="progress-track"><span style={{width:`${progress}%`}}/></div><div className="run-stats"><Budget value={run.totalJobs} label="durable jobs"/><Budget value={run.queuedJobs} label="queued"/><Budget value={run.completedJobs} label="completed"/><Budget value={run.failedJobs} label="failed"/></div><div className="run-meta"><span>Stage <b>{run.stage.replaceAll("_"," ")}</b></span><span>Bounds <b>{run.maxAppearancesPerSeed} appearances · {run.maxTracklists} tracklists</b></span><span>Browser <b>headless unless explicitly opened</b></span><span>Policy <b>round-robin seeds</b></span></div></div>;
}

function audioProvider(url: string): "youtube" | "bandcamp" | "soundcloud" {
  const host = new URL(url).hostname.toLowerCase();
  if (["youtube.com","www.youtube.com","m.youtube.com","youtu.be"].includes(host)) return "youtube";
  if (host === "soundcloud.com" || host.endsWith(".soundcloud.com")) return "soundcloud";
  if (host === "bandcamp.com" || host.endsWith(".bandcamp.com")) return "bandcamp";
  throw new Error("Use a YouTube, Bandcamp, or SoundCloud HTTPS URL.");
}

function ResultsPage({onPlay}:{onPlay:(item:Recommendation)=>void}) {
  const client = useQueryClient();
  const results = useQuery({queryKey:["recommendations"],queryFn:listRecommendations});
  const [openId,setOpenId] = useState<number>();
  const [audio,setAudio] = useState<Record<number,string>>({});
  const feedback = useMutation({mutationFn:({id,value}:{id:number;value?:"saved"|"rejected"|"dismissed"})=>setRecommendationFeedback(id,value),onSuccess:()=>client.invalidateQueries({queryKey:["recommendations"]})});
  const attach = useMutation({mutationFn:({id,url}:{id:number;url:string})=>attachAudioSource(id,audioProvider(url),url),onSuccess:(_,variables)=>{setAudio(values=>({...values,[variables.id]:""}));client.invalidateQueries({queryKey:["recommendations"]});}});
  const wrongVersion = useMutation({mutationFn:markAudioSourceWrongVersion,onSuccess:()=>client.invalidateQueries({queryKey:["recommendations"]})});
  const exportCsv = useMutation({mutationFn:exportShortlist});
  const error = results.error ?? feedback.error ?? attach.error ?? wrongVersion.error ?? exportCsv.error;
  const active = results.data?.filter(item => !["rejected","dismissed"].includes(item.disposition ?? "")) ?? [];
  const removed = results.data?.filter(item => ["rejected","dismissed"].includes(item.disposition ?? "")) ?? [];
  return <section className="page"><div className="page-title"><div><p className="eyebrow">EVIDENCE-BACKED SHORTLIST</p><h1>Recommendations</h1><p>Ranked deterministically from the fetched sets. Save, reject, inspect evidence, and attach the exact recording.</p></div><button className="primary" onClick={()=>exportCsv.mutate()} disabled={exportCsv.isPending}><Download size={17}/>{exportCsv.isPending ? "Exporting…" : "Export saved CSV"}</button></div>
    {error&&<div className="error"><X size={18}/>{String(error)}</div>}
    <div className="result-list">{active.map((item,index)=><RecommendationCard key={item.id} item={item} rank={index+1} expanded={openId===item.id} audioUrl={audio[item.id]??""} busy={feedback.isPending||attach.isPending||wrongVersion.isPending} onToggle={()=>setOpenId(openId===item.id?undefined:item.id)} onAudioChange={url=>setAudio(values=>({...values,[item.id]:url}))} onAttach={()=>attach.mutate({id:item.sourceTrackId,url:audio[item.id]??""})} onPlay={()=>onPlay(item)} onFeedback={value=>feedback.mutate({id:item.sourceTrackId,value})} onWrongVersion={()=>wrongVersion.mutate(item.sourceTrackId)}/>)}</div>
    {!results.isLoading&&!results.data?.length&&<Empty title="No ranked candidates yet" body="Complete a discovery run first. Partial source failures remain visible on the run while usable evidence is retained." icon={<Sparkles/>} action="Refresh results" onAction={()=>results.refetch()}/>}
    {removed.length>0&&<details className="removed-results"><summary>{removed.length} rejected or globally dismissed track{removed.length===1?"":"s"}</summary><div>{removed.map(item=><div key={item.id}><span><b>{item.title}</b><small>{item.artist} · {item.disposition === "dismissed" ? "Dismissed globally" : "Rejected for this playlist"}</small></span><button onClick={()=>feedback.mutate({id:item.sourceTrackId})}><RotateCcw size={14}/>Undo</button></div>)}</div></details>}
  </section>;
}

function RecommendationCard({item,rank,expanded,audioUrl,busy,onToggle,onAudioChange,onAttach,onPlay,onFeedback,onWrongVersion}:{item:Recommendation;rank:number;expanded:boolean;audioUrl:string;busy:boolean;onToggle:()=>void;onAudioChange:(url:string)=>void;onAttach:()=>void;onPlay:()=>void;onFeedback:(value?:"saved"|"rejected"|"dismissed")=>void;onWrongVersion:()=>void}) {
  return <article className="result-card panel"><span className="result-rank">{String(rank).padStart(2,"0")}</span><div className="result-main"><h2>{item.title}{item.version&&<small> · {item.version}</small>}</h2><p>{item.artist}</p><div className="evidence-summary">Found in <b>{item.setCount}</b> fetched set{item.setCount===1?"":"s"} containing <b>{item.seedCount}</b> seed{item.seedCount===1?"":"s"}; <b>{item.djCount}</b> identified DJ{item.djCount===1?"":"s"}; adjacent in <b>{item.adjacentCount}</b> set{item.adjacentCount===1?"":"s"}.</div><button className="evidence-toggle" onClick={onToggle}>Evidence & source {expanded?"−":"+"}</button>{expanded&&<div className="evidence-drawer">{item.evidenceUrls.map(url=><a key={url} href={url} target="_blank" rel="noreferrer">Open supporting set <ExternalLink size={12}/></a>)}<div className="attach-row"><input value={audioUrl} onChange={event=>onAudioChange(event.target.value)} placeholder="Exact YouTube, Bandcamp, or SoundCloud URL"/><button onClick={onAttach} disabled={!audioUrl||busy}>{item.sourceUrl?"Replace source":"Attach source"}</button></div>{item.sourceUrl&&<div className="source-controls"><span>{item.sourceProvider} · checked {item.playbackStatus ?? "unknown"}</span><button onClick={onWrongVersion} disabled={busy}>Wrong version</button></div>}<button className="dismiss-link" onClick={()=>onFeedback("dismissed")} disabled={busy}>Dismiss this recording globally</button></div>}</div><strong className="score">{item.score.toFixed(1)}<small>score</small></strong><div className="result-actions"><button onClick={onPlay} disabled={!item.sourceUrl}><Play size={15}/>Audition</button><button className={item.disposition==="saved"?"saved":""} onClick={()=>onFeedback(item.disposition==="saved"?undefined:"saved")} disabled={busy}><Heart size={15}/>{item.disposition==="saved"?"Saved":"Save"}</button><button onClick={()=>onFeedback("rejected")} disabled={busy}><X size={15}/>Reject</button></div></article>;
}

function Budget({value,label}:{value:string|number;label:string}){return <div className="budget"><strong>{value}</strong><span>{label}</span></div>}

function SettingsPage(){ return <section className="page narrow"><p className="eyebrow">CONFIGURATION</p><h1>Settings</h1><div className="panel settings"><h2>Source connections</h2><Setting title="1001Tracklists adapter" text="Implemented · live challenged extraction still needs verification."/><Setting title="Audio sources" text="Attach verified YouTube, Bandcamp, or SoundCloud URLs per recommendation."/><Setting title="Browser session" text="Installed Chrome · dedicated persistent profile"/></div></section> }
function Setting({title,text}:{title:string;text:string}){return <div className="setting"><div><b>{title}</b><p>{text}</p></div></div>}
function Stat({value,label,muted}:{value:string|number;label:string;muted?:boolean}){return <div className={`stat ${muted?"muted":""}`}><strong>{value}</strong><span>{label}</span></div>}
function Info({n,title,text}:{n:string;title:string;text:string}){return <div className="info"><span>{n}</span><h3>{title}</h3><p>{text}</p></div>}
function Empty({title,body,icon,action,onAction}:{title:string;body:string;icon:React.ReactNode;action:string;onAction:()=>void}){return <div className="empty"><span>{icon}</span><h2>{title}</h2><p>{body}</p><button onClick={onAction}>{action}</button></div>}
function youtubeEmbedUrl(sourceUrl?: string): string | undefined {
  if (!sourceUrl) return;
  try {
    const url = new URL(sourceUrl);
    let videoId: string | null | undefined;
    if (url.hostname === "youtu.be") videoId = url.pathname.split("/").filter(Boolean)[0];
    if (["youtube.com","www.youtube.com","m.youtube.com"].includes(url.hostname)) {
      videoId = url.searchParams.get("v") ?? (url.pathname.startsWith("/shorts/") ? url.pathname.split("/")[2] : undefined);
    }
    if (!videoId || !/^[A-Za-z0-9_-]{6,20}$/.test(videoId)) return;
    return `https://www.youtube-nocookie.com/embed/${videoId}?autoplay=1`;
  } catch {
    return;
  }
}

function Player({recommendation,onExport}:{recommendation?:Recommendation;onExport:()=>void}) {
  const embedUrl = recommendation?.sourceProvider === "youtube" ? youtubeEmbedUrl(recommendation.sourceUrl) : undefined;
  return <footer className="player"><div className="cover"><Music2/></div><div><b>{recommendation?.title??"Nothing playing"}</b><small>{recommendation?recommendation.artist:"Select a recommendation to audition"}</small></div>{embedUrl?<iframe title="Persistent YouTube audition player" src={embedUrl} allow="autoplay; encrypted-media" referrerPolicy="strict-origin-when-cross-origin"/>:<div className="player-line"/>}<a className={!recommendation?.sourceUrl?"disabled":""} href={recommendation?.sourceUrl} target="_blank" rel="noreferrer"><Headphones size={18}/>{embedUrl?"Open externally":recommendation?.sourceUrl?`Open ${recommendation.sourceProvider}`:"Open externally"}</a><button onClick={onExport}><Download size={18}/>Export</button></footer>;
}
