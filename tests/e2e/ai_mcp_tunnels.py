# End-to-end check of tunnels, AI chat (mock LLM) and MCP. Needs: a TT server on 127.0.0.1:8443 with an empty data dir,
# an sshd on 127.0.0.1:2222 with user ttest / password Pass-12345!x, `pip install websockets`. Run: TT_BIN=./target/debug/tt python3 tests/e2e/ai_mcp_tunnels.py
import json, asyncio, urllib.request, http.cookiejar, websockets, threading, http.server, subprocess, time
B="http://127.0.0.1:8443"
cj=http.cookiejar.CookieJar(); op=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
def call(m,p,body=None,headers=None):
    h={"Content-Type":"application/json"} if body is not None else {}
    h.update(headers or {})
    r=urllib.request.Request(B+p,data=json.dumps(body).encode() if body is not None else None,method=m,headers=h)
    try: x=op.open(r); b=x.read(); return x.status,(json.loads(b) if b else None)
    except urllib.error.HTTPError as e: return e.code,e.read().decode()
# --- mock LLM + target web server
seen=[]
class LLM(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        seen.append((self.path,dict(self.headers),body))
        txt="Rode:\n```\nuptime\n```\ne cuidado:\n```\nreboot\n```"
        if self.path.endswith('/messages'): out={"content":[{"type":"text","text":txt}]}
        else: out={"choices":[{"message":{"content":txt}}]}
        b=json.dumps(out).encode(); self.send_response(200); self.send_header('Content-Type','application/json'); self.end_headers(); self.wfile.write(b)
class Web(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(b"TUNNEL_TARGET_OK")
for port,cls in ((9911,LLM),(9922,Web)):
    s=http.server.ThreadingHTTPServer(("127.0.0.1",port),cls); threading.Thread(target=s.serve_forever,daemon=True).start()
ok=lambda c,msg: print(("PASS " if c else "FAIL ")+msg)
call("POST","/api/setup",{"username":"admin","password":"correct-horse-9"})
s,r=call("POST","/api/hosts",{"name":"srv","folder":"Lab","host":"127.0.0.1","port":2222,"username":"ttest","auth_kind":"password","profile_id":"debian","secret":"Pass-12345!x"}); hid=r["id"]
cookie="; ".join(f"{c.name}={c.value}" for c in cj)
async def main():
    async with websockets.connect(f"ws://127.0.0.1:8443/api/term/{hid}",additional_headers={"Cookie":cookie,"Origin":B}) as ws:
        async def drain(t):
            end=asyncio.get_event_loop().time()+t; got=[]
            while asyncio.get_event_loop().time()<end:
                try: m=await asyncio.wait_for(ws.recv(),0.3)
                except asyncio.TimeoutError: continue
                if not isinstance(m,bytes):
                    j=json.loads(m); got.append(j)
                    if j["t"]=="hostkey": await ws.send(json.dumps({"t":"trust"}))
            return got
        await ws.send(json.dumps({"t":"resize","cols":100,"rows":30})); await drain(3)
        await ws.send(b"echo MCP_MARK_123; echo password=hunter2\n"); await drain(1.5)
        loop=asyncio.get_running_loop()
        # ---------- tunnels (blocking calls in executor)
        def tunnels():
            print("-- tunnels")
            s,_=call("POST","/api/tunnels",{"host_id":hid,"name":"x","kind":"local","listen_host":"0.0.0.0","listen_port":18080,"dest_host":"127.0.0.1","dest_port":9922}); ok(s==400,f"public bind rejected ({s})")
            s,_=call("POST","/api/tunnels",{"host_id":hid,"name":"x","kind":"local","listen_port":80,"dest_host":"127.0.0.1","dest_port":9922}); ok(s==400,f"low port rejected ({s})")
            s,t1=call("POST","/api/tunnels",{"host_id":hid,"name":"web","kind":"local","listen_port":18080,"dest_host":"127.0.0.1","dest_port":9922})
            s,t2=call("POST","/api/tunnels",{"host_id":hid,"name":"socks","kind":"dynamic","listen_port":11080})
            for t in (t1,t2):
                s,r=call("POST",f"/api/tunnels/{t['id']}/start"); ok(s==200,f"start {t['id']}: {r}")
            out=subprocess.run(["curl","-s","--max-time","5","http://127.0.0.1:18080/"],capture_output=True,text=True).stdout; ok("TUNNEL_TARGET_OK" in out,f"local forward -> {out!r}")
            out=subprocess.run(["curl","-s","--max-time","5","--socks5-hostname","127.0.0.1:11080","http://127.0.0.1:9922/"],capture_output=True,text=True).stdout; ok("TUNNEL_TARGET_OK" in out,f"socks5 -> {out!r}")
            s,l=call("GET","/api/tunnels"); ok(all(x["running"] for x in l),"both running")
            for t in (t1,t2): call("POST",f"/api/tunnels/{t['id']}/stop")
            time.sleep(.3); out=subprocess.run(["curl","-s","--max-time","2","http://127.0.0.1:18080/"],capture_output=True,text=True).stdout; ok(out=="","stopped tunnel no longer answers")
        await loop.run_in_executor(None,tunnels)
        # ---------- AI
        def ai():
            print("-- ai")
            s,_=call("PUT","/api/ai/settings",{"provider":"anthropic","base_url":"http://127.0.0.1:9911","model":"m1","api_key":"sk-test-key-123456789012345"}); ok(s==200,"ai settings")
            s,g=call("GET","/api/ai/settings"); ok(g["has_key"] and "api_key" not in g and "sk-test" not in json.dumps(g),"key never returned")
            s,r=call("POST","/api/ai/chat",{"host_id":hid,"messages":[{"role":"user","content":"o que houve?"}],"screen":"login ok\nenable secret 9 $9$SUPERSECRET\npassword=hunter2\nuptime 3d"})
            ok(s==200 and r["commands"][0]["readonly"] is True and r["commands"][1]["readonly"] is False,f"commands flagged: {r.get('commands') if isinstance(r,dict) else r}")
            p,h,b=seen[-1]; sent=json.dumps(b)
            ok("SUPERSECRET" not in sent and "hunter2" not in sent and "[REDACTED]" in sent,"screen redacted before reaching the LLM")
            ok(h.get("x-api-key")=="sk-test-key-123456789012345" and "untrusted" in b["system"] and "debian" in b["system"].lower(),"anthropic call shape + system prompt")
            call("PUT","/api/ai/settings",{"provider":"openai","base_url":"http://127.0.0.1:9911","model":"m2","api_key":"","send_screen":False})
            s,r=call("POST","/api/ai/chat",{"host_id":hid,"messages":[{"role":"user","content":"oi"}],"screen":"SHOULD_NOT_BE_SENT"})
            p,h,b=seen[-1]; ok(p.endswith("/chat/completions") and "SHOULD_NOT_BE_SENT" not in json.dumps(b) and h.get("authorization","").startswith("Bearer sk-test"),"openai shape + send_screen=false respected")
            call("POST","/api/users",{"username":"tecnico","password":"tecnico-pass-1"})
        await loop.run_in_executor(None,ai)
        # ---------- MCP
        def mcp(method,params=None,token=None,id=1):
            body={"jsonrpc":"2.0","id":id,"method":method}
            if params is not None: body["params"]=params
            r=urllib.request.Request(B+"/mcp",data=json.dumps(body).encode(),method="POST",headers={"Content-Type":"application/json",**({"Authorization":"Bearer "+token} if token else {})})
            try: return json.loads(urllib.request.urlopen(r).read())
            except urllib.error.HTTPError as e: return e.code
        def tool(name,args,token):
            r=mcp("tools/call",{"name":name,"arguments":args},token); return r["result"]["content"][0]["text"],r["result"]["isError"]
        state={}
        def setup_tokens():
            print("-- mcp")
            _,a=call("POST","/api/tokens",{"name":"ide","scope":"exec"}); _,b=call("POST","/api/tokens",{"name":"ro","scope":"read"}); state["x"]=a["token"]; state["r"]=b["token"]
            ok(mcp("initialize",{"protocolVersion":"2025-06-18"})==401,"no token -> 401")
            r=mcp("initialize",{"protocolVersion":"2025-06-18"},state["x"]); ok(r["result"]["serverInfo"]["name"]=="tt","initialize")
            names=[t["name"] for t in mcp("tools/list",None,state["x"])["result"]["tools"]]; ok("run_readonly" in names and "suggest_command" in names,f"tools/list {names}")
            t,e=tool("list_hosts",{},state["x"]); ok("srv" in t and "Pass-12345" not in t,"list_hosts without secrets")
            t,e=tool("read_terminal",{"host_id":hid,"lines":30},state["x"]); ok("MCP_MARK_123" in t and "hunter2" not in t and "\x1b" not in t,f"read_terminal clean+redacted: {t[-120:]!r}")
            t,e=tool("suggest_command",{"host_id":hid,"command":"uptime","reason":"ver carga"},state["x"]); ok(not e and "NOT executed" in t,"suggest_command queued")
            t,e=tool("run_readonly",{"host_id":hid,"command":"uname -a"},state["x"]); ok(not e and "Linux" in t,f"run_readonly ok: {t[:60]!r}")
            t,e=tool("run_readonly",{"host_id":hid,"command":"cat /etc/shadow"},state["x"]); ok(e and "denied" in t,"run_readonly denies non-allowlisted")
            t,e=tool("run_readonly",{"host_id":hid,"command":"uptime; reboot"},state["x"]); ok(e and "denied" in t,"run_readonly denies chaining")
            t,e=tool("run_readonly",{"host_id":hid,"command":"uptime"},state["r"]); ok(e and "read scope" in t,"read-scope token cannot exec")
            s,a=call("GET","/api/audit"); ok(any(x["action"]=="run_readonly_denied" for x in a),"audit log records denial")
        await loop.run_in_executor(None,setup_tokens)
        got=await drain(1.5); ok(any(g["t"]=="suggestion" and g["command"]=="uptime" and g["readonly"] for g in got),"UI receives suggestion over websocket")
    # stdio bridge
    p=subprocess.run([__import__("os").environ.get("TT_BIN","./target/debug/tt"),"mcp","--url",B,"--token",state["x"]],input='{"jsonrpc":"2.0","id":1,"method":"tools/list"}\n{"jsonrpc":"2.0","method":"notifications/initialized"}\n',capture_output=True,text=True,timeout=20)
    ok('"run_readonly"' in p.stdout and p.stdout.count("\n")==1,"stdio bridge relays request, swallows notification")
asyncio.run(main())
