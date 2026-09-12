"""Generate a self-contained HTML adjudication page from the review queue.

Shows the PRIMARY material for each disagreement — full claim, full source text
with the Opus quote highlighted in place — plus the prior and Opus verdicts as
bare data points. No model summary. You decide from the source; the page exports
a {id: verdict} JSON to feed back.

    python3 build_review.py --out review.html
"""
from __future__ import annotations
import argparse, json, html

SCR = "/tmp/claude-1000/-var-home-louie-Projects-Volunteering-Consulting-SP42/d2a59721-e423-4fec-a993-95ebd3386eaf/scratchpad"
REPO = f"{SCR}/citation-eval-corpus"


def assemble():
    import os
    queue = json.load(open(f"{SCR}/review-queue.json"))
    cands = {c["id"]: c for c in json.load(open(f"{SCR}/gold-candidates.json"))}
    wafer_fetched = json.load(open(f"{SCR}/wafer-fetched.json"))
    alex = {c["id"]: c for c in json.load(open(f"{REPO}/corpora/alex-189/cases.json"))}
    citoid = json.load(open(f"{SCR}/citoid.json")) if os.path.exists(f"{SCR}/citoid.json") else {}

    def full_source(cid, corpus):
        if corpus == "wafer-en":
            return wafer_fetched.get(cid, {}).get("text", "")
        return alex.get(cid, {}).get("source_text") or ""

    out = []
    for group, kind in (("judgment_disagreements", "judgment"),
                        ("extraction_source_unavailable", "extraction")):
        for r in queue.get(group, []):
            c = cands.get(r["id"], {})
            container = ""
            if r["corpus"] == "alex-189":
                container = (alex.get(r["id"], {}).get("claim_context", {}) or {}).get("container") or ""
            out.append({
                "id": r["id"], "corpus": r["corpus"], "kind": kind,
                "prior": r["prior"], "opus": r["opus"], "quote": r.get("quote", ""),
                "fetch_status": r.get("fetch_status"),
                "claim": c.get("claim", ""), "url": c.get("source_url", ""),
                "container": container,
                "meta": citoid.get(r["id"]),  # the bibliographic context the model had
                "quote_located": r.get("quote_located"),
                "source": full_source(r["id"], r["corpus"]),
            })
    return out


HTML = r"""<!doctype html><html lang=en><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>Citation adjudication</title>
<style>
:root{--bg:#fbfbf9;--fg:#1a1a1a;--mut:#6b6b6b;--line:#e3e3dd;--acc:#2b6cb0;--mark:#fde68a}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);
font:16px/1.55 ui-serif,Georgia,"Times New Roman",serif}
header{position:sticky;top:0;z-index:5;background:#fff;border-bottom:1px solid var(--line);
padding:.6rem 1rem;display:flex;gap:1rem;align-items:center;flex-wrap:wrap;
font-family:ui-sans-serif,system-ui,sans-serif;font-size:14px}
header b{font-size:15px}.spacer{flex:1}
button,select{font:inherit;padding:.35rem .7rem;border:1px solid var(--line);border-radius:6px;background:#fff;cursor:pointer}
button.primary{background:var(--acc);color:#fff;border-color:var(--acc)}
#prog{color:var(--mut)}
main{max-width:900px;margin:0 auto;padding:1rem}
.card{background:#fff;border:1px solid var(--line);border-radius:10px;padding:1.1rem 1.2rem;margin:1rem 0}
.card.decided{border-left:4px solid #2f855a}
.meta{font-family:ui-sans-serif,system-ui,sans-serif;font-size:12.5px;color:var(--mut);
display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;margin-bottom:.6rem}
.chip{padding:.1rem .5rem;border-radius:999px;border:1px solid var(--line);background:#f4f4ef}
.chip.prior{background:#eef2ff}.chip.opus{background:#fef3f2}.chip.kind{background:#f0fdf4}
.lab{font-family:ui-sans-serif,system-ui,sans-serif;font-size:11.5px;letter-spacing:.06em;
text-transform:uppercase;color:var(--mut);margin:.8rem 0 .25rem}
.claim{font-size:17px}
.src{white-space:pre-wrap;max-height:300px;overflow:auto;background:#fcfcfa;border:1px solid var(--line);
border-radius:8px;padding:.7rem .8rem;font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace}
.src mark{background:var(--mark);padding:0 1px}
.quote{font-style:italic;color:#444;border-left:3px solid var(--mark);padding-left:.6rem;margin:.3rem 0}
.hint{font-family:ui-sans-serif,system-ui,sans-serif;font-weight:400;font-size:11px;color:var(--mut);text-transform:none;letter-spacing:0}
.fn{font-family:ui-sans-serif,system-ui,sans-serif;font-size:11px;color:var(--acc);border:1px solid var(--acc);border-radius:4px;padding:0 .25rem;vertical-align:super;line-height:1}
.src.ctx{max-height:170px;font-family:ui-serif,Georgia,serif;font-size:14px}
sup.cit{color:var(--acc);font-weight:700;font-size:.8em}
.meta-box{background:#eef4fb;border:1px solid #d6e3f0;border-radius:8px;padding:.5rem .7rem;font-family:ui-sans-serif,system-ui,sans-serif;font-size:13px;line-height:1.5}
.meta-box.muted{color:var(--mut);background:#f6f6f3}
.meta-box .mk{display:inline-block;min-width:5.5rem;color:var(--mut);font-size:11px;text-transform:uppercase;letter-spacing:.04em}
.chip.warn{background:#fef2f2;border-color:#f3c0c0;color:#b42318}
.url{font-family:ui-monospace,monospace;font-size:12px;word-break:break-all}
.url a{color:var(--acc)}
.dec{display:flex;gap:.4rem;flex-wrap:wrap;margin-top:.7rem;font-family:ui-sans-serif,system-ui,sans-serif;font-size:13px}
.dec label{border:1px solid var(--line);border-radius:6px;padding:.3rem .6rem;cursor:pointer;user-select:none}
.dec input{margin-right:.35rem}
.dec label:has(input:checked){background:var(--acc);color:#fff;border-color:var(--acc)}
.note{margin-top:.5rem;width:100%;font:inherit;font-size:13px;padding:.4rem .6rem;border:1px solid var(--line);border-radius:6px}
dialog{border:1px solid var(--line);border-radius:10px;max-width:700px;width:90%}
textarea{width:100%;height:240px;font:12px/1.45 ui-monospace,monospace}
.hide{display:none}
#rulesBtn{position:fixed;right:1rem;bottom:1rem;z-index:20;border-radius:999px;
box-shadow:0 2px 8px rgba(0,0,0,.15);font-family:ui-sans-serif,system-ui,sans-serif}
#rules{position:fixed;right:1rem;bottom:3.6rem;z-index:20;width:360px;max-width:92vw;
max-height:72vh;overflow:auto;background:#fff;border:1px solid var(--line);border-radius:10px;
box-shadow:0 6px 24px rgba(0,0,0,.18);padding:.9rem 1rem;
font-family:ui-sans-serif,system-ui,sans-serif;font-size:13px;line-height:1.5}
#rules h4{margin:.2rem 0 .5rem;font-size:13px}
#rules .v{font-weight:700}
#rules dt{font-weight:700;margin-top:.55rem}
#rules dd{margin:.1rem 0 0}
#rules .sub{color:var(--mut);font-size:12px;margin:.6rem 0 .2rem;text-transform:uppercase;letter-spacing:.05em}
#rules ul{margin:.2rem 0;padding-left:1.1rem}
#rules .only{background:#f0fdf4;border:1px solid var(--line);border-radius:6px;padding:.4rem .5rem;margin-top:.6rem}
</style></head><body>
<header>
<b>Citation adjudication</b>
<span id=prog></span>
<span class=spacer></span>
<label style="display:flex;gap:.3rem;align-items:center"><input type=checkbox id=undec> undecided only</label>
<select id=fc><option value=all>all corpora</option><option value=wafer-en>wafer-en (silver→gold)</option><option value=alex-189>alex-189 (re-check)</option></select>
<select id=fk><option value=all>all kinds</option><option value=judgment>judgment</option><option value=extraction>extraction (src-unavail)</option></select>
<button class=primary id=exp>Export decisions</button>
</header>
<main id=app></main>
<dialog id=dlg><h3 style="font-family:sans-serif">Decisions JSON</h3>
<p style="font-family:sans-serif;font-size:13px;color:#6b6b6b">Copy this and paste it back in chat.</p>
<textarea id=out readonly></textarea><div style="margin-top:.6rem;text-align:right">
<button id=copy class=primary>Copy</button> <button onclick="dlg.close()">Close</button></div></dialog>
<button id=rulesBtn>📖 Rules</button>
<aside id=rules class=hide>
<h4>Verdict definitions <span style="font-weight:400;color:#6b6b6b">(SP42 verifier rules)</span></h4>
<dl>
<dt class=v>supported</dt><dd>The source contains <b>all</b> of the claim's specific assertions (paraphrase OK if substance matches). Needs a verbatim supporting span.</dd>
<dt class=v>partial</dt><dd>The source addresses the claim but contains only <b>some</b> of its assertions, <b>or</b> asserts it only with hedged/uncertain language. Needs a verbatim span.</dd>
<dt class=v>not_supported</dt><dd>The source addresses the topic but <b>contradicts</b> the claim, or has <b>no evidence</b> for its specific assertions. (Also: if no verbatim supporting span exists, it is not_supported.)</dd>
<dt class=v>source_unavailable</dt><dd>STEP&nbsp;1 failed — no usable article body: a catalog page (Google Books/WorldCat/JSTOR preview), paywall, login wall, 404, cookie/consent notice, anti-bot challenge, or bibliographic metadata only. <em>Excerpts, gaps, "…", or brevity are NORMAL — not unavailable.</em></dd>
</dl>
<div class=sub>Judging (STEP 2)</div>
<ul>
<li>Judge using <b>only</b> the source text — no outside knowledge.</li>
<li><b>Dates:</b> must appear in some form; equivalents count ("Wednesday" = that day; "7 Jan 2026" = "7 January 2026").</li>
<li><b>Numbers / names / quotes:</b> the specific value must be present, or a directly equivalent paraphrase.</li>
<li>Accept paraphrase and direct implication — <b>not</b> speculative inference or logical leaps.</li>
<li><b>Definitive vs hedged:</b> a claim stated as fact needs definitive source text; hedged source ("it is believed") → partial.</li>
<li>Transliteration variants of one name ("Chekhov"/"Tchekhov") are equal, not errors.</li>
</ul>
<div class=only><b>drop</b> — not a verdict. Mark a broken case (malformed/mis-extracted claim, or an extraction artifact). Excluded from the corpus, not labeled.</div>
</aside>
<script>
const DATA = JSON.parse(__DATA__);
const KEY = "cit-adjudication-v1";
const store = JSON.parse(localStorage.getItem(KEY) || "{}");
const VERDICTS = ["supported","partial","not_supported","source_unavailable","drop"];
const esc = s => (s||"").replace(/[&<>]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;"}[c]));
function highlight(src, q){
  if(!q) return esc(src);
  const i = src.toLowerCase().indexOf(q.slice(0,60).toLowerCase());
  if(i<0) return esc(src);
  const j=i+q.length;
  return esc(src.slice(0,i))+"<mark>"+esc(src.slice(i,j))+"</mark>"+esc(src.slice(j));
}
// Highlight the cited claim inside its article paragraph and emphasize [N] footnote markers.
function markCtx(container, claim){
  return highlight(container, claim).replace(/\[(\d+)\]/g, "<sup class=cit>[$1]</sup>");
}
// The bibliographic metadata the model was given (context-only; not a grounding quote).
function metaBlock(m){
  if(!m) return `<div class=lab>Source metadata <span class=hint>(model context)</span></div><div class="meta-box muted">Citoid returned no metadata — the model had none either.</div>`;
  const rows=[["publication",m.publication],["published",m.published],["author",m.author],["title",m.title]].filter(x=>x[1]);
  if(!rows.length) return `<div class=lab>Source metadata <span class=hint>(model context)</span></div><div class="meta-box muted">No metadata fields.</div>`;
  return `<div class=lab>Source metadata <span class=hint>(context the model had — not quotable as grounding)</span></div>
    <div class=meta-box>${rows.map(([k,v])=>`<div><span class=mk>${k}</span> ${esc(v)}</div>`).join("")}</div>`;
}
function render(){
  const undec = document.getElementById("undec").checked;
  const fk = document.getElementById("fk").value;
  const fc = document.getElementById("fc").value;
  const app = document.getElementById("app"); app.innerHTML="";
  let shown=0;
  DATA.forEach((d,i)=>{
    if(fk!=="all" && d.kind!==fk) return;
    if(fc!=="all" && d.corpus!==fc) return;
    const decided = !!store[d.id];
    if(undec && decided) return;
    shown++;
    const el=document.createElement("div"); el.className="card"+(decided?" decided":"");
    el.innerHTML=`<div class=meta><span>#${i}</span><span class=chip>${d.corpus}</span>
      <span class=chip kind>${d.kind}${d.fetch_status?(" · "+d.fetch_status):""}</span>
      <span class=chip prior>prior: ${d.prior}</span><span class=chip opus>Opus: ${d.opus}</span>
      ${(d.opus==="supported"||d.opus==="partial")&&d.quote_located===false?'<span class="chip warn">⚠ quote not located in source</span>':""}
      <span style="opacity:.5">${d.id}</span></div>
      <div class=lab>Cited claim <span class=hint>— the footnote backs this statement</span></div><div class=claim>${esc(d.claim)} <span class=fn>[cite]</span></div>
      ${d.container && d.container!==d.claim?`<div class=lab>In the article <span class=hint>(claim highlighted; footnote markers in blue)</span></div><div class="src ctx">${markCtx(d.container,d.claim)}</div>`:""}
      ${d.quote?`<div class=lab>Opus supporting quote</div><div class=quote>${esc(d.quote)}</div>`:""}
      ${metaBlock(d.meta)}
      <div class=lab>Source <span class=url>· <a href="${esc(d.url)}" target=_blank rel=noopener>${esc(d.url)}</a></span></div>
      <div class=src>${highlight(d.source,d.quote)}</div>
      <div class=dec>${VERDICTS.map(v=>`<label><input type=radio name="d_${d.id}" value="${v}" ${store[d.id]&&store[d.id].v===v?"checked":""}>${v}</label>`).join("")}</div>
      <input class=note placeholder="note (optional)" value="${store[d.id]?esc(store[d.id].n||""):""}">`;
    el.querySelectorAll(`input[name="d_${d.id}"]`).forEach(r=>r.addEventListener("change",()=>{
      store[d.id]=store[d.id]||{}; store[d.id].v=r.value; save(); el.classList.add("decided");
    }));
    el.querySelector(".note").addEventListener("input",e=>{store[d.id]=store[d.id]||{};store[d.id].n=e.target.value;save();});
    app.appendChild(el);
  });
  prog(shown);
}
function save(){localStorage.setItem(KEY,JSON.stringify(store));prog();}
function prog(shown){
  const total=DATA.length, done=Object.values(store).filter(x=>x&&x.v).length;
  document.getElementById("prog").textContent=`${done}/${total} decided`+(shown!=null?` · ${shown} shown`:"");
}
document.getElementById("undec").addEventListener("change",render);
document.getElementById("fk").addEventListener("change",render);
document.getElementById("fc").addEventListener("change",render);
document.getElementById("exp").addEventListener("click",()=>{
  const o={}; for(const id in store){if(store[id]&&store[id].v)o[id]=store[id].n?{v:store[id].v,n:store[id].n}:store[id].v;}
  document.getElementById("out").value=JSON.stringify(o,null,1);
  document.getElementById("dlg").showModal();
});
document.getElementById("copy").addEventListener("click",()=>{navigator.clipboard.writeText(document.getElementById("out").value);});
const rules=document.getElementById("rules");
document.getElementById("rulesBtn").addEventListener("click",()=>rules.classList.toggle("hide"));
document.addEventListener("keydown",e=>{if(e.key==="?"&&e.target.tagName!=="INPUT"&&e.target.tagName!=="TEXTAREA")rules.classList.toggle("hide");});
render();
</script></body></html>"""


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)
    data = assemble()
    payload = json.dumps(json.dumps(data, ensure_ascii=False)).replace("</", "<\\/")
    open(args.out, "w", encoding="utf-8").write(HTML.replace("__DATA__", payload))
    print(f"wrote {args.out}: {len(data)} cases "
          f"({sum(1 for d in data if d['kind']=='judgment')} judgment, "
          f"{sum(1 for d in data if d['kind']=='extraction')} extraction)")


if __name__ == "__main__":
    main()
