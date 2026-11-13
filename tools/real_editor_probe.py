"""Owned editors with isolated profiles or a dedicated Cursor development window."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import subprocess
import threading
import time
import tempfile
from pathlib import Path

try:
    from . import superset_input_probe as probe
except ImportError:
    import superset_input_probe as probe


HTML = r'''<!doctype html><meta charset="utf-8"><title>Upyr owned probe</title>
<style>body{font:20px system-ui;margin:40px}textarea{width:90%;height:400px;font:22px monospace}</style>
<h1>Upyr owned typing probe</h1><textarea id="editor" spellcheck="false" autocorrect="off" autocapitalize="off" autocomplete="off" aria-label="Upyr owned test editor"></textarea>
<script>
const field=document.querySelector('#editor'); let nonce='', serial=0;
field.focus();
async function publish(){
 const state={nonce,text:field.value,ready:document.hasFocus()&&document.activeElement===field,
  enters:(field.value.match(/\n/g)||[]).length,observed_at_ms:Date.now(),serial:++serial};
 await fetch('state',{method:'POST',body:JSON.stringify(state)});
}
field.addEventListener('input',publish); window.addEventListener('focus',publish); window.addEventListener('blur',publish);
async function tick(){try{
 const control=await (await fetch('control',{cache:'no-store'})).json();
 if(control.nonce!==nonce && document.hasFocus()&&document.activeElement===field){
  field.value=control.seed||''; field.setSelectionRange(field.value.length,field.value.length); nonce=control.nonce;
 }
 await publish();
}finally{setTimeout(tick,10)}} tick();
</script>'''

EXTENSION = r'''
const vscode=require('vscode'),fs=require('fs'),path=require('path');
exports.activate=async function(context){
 const root=__dirname, file=path.join(root,'Upyr-probe-'+path.basename(root)+'.txt');
 const document=await vscode.workspace.openTextDocument(file);
 await vscode.window.showTextDocument(document,{preview:false,preserveFocus:false});
 let nonce='',busy=false;
 const tracing=fs.existsSync(path.join(root,'trace-enabled'));
 function record(kind){
  if(!tracing)return;
  const editor=vscode.window.activeTextEditor;
  if(editor?.document!==document)return;
  fs.appendFileSync(path.join(root,'editor-events.jsonl'),JSON.stringify({kind,at_ms:Date.now(),nonce,text:document.getText(),
   selections:editor.selections.map(s=>[document.offsetAt(s.anchor),document.offsetAt(s.active)])})+'\n');
 }
 function ready(){return vscode.window.state.focused && vscode.window.activeTextEditor?.document.uri.fsPath===file;}
 function publish(){
  const text=document.getText();
  const state={pid:process.pid,ready:ready(),nonce,text,enters:(text.match(/\n/g)||[]).length,observed_at_ms:Date.now()};
  const tmp=path.join(root,'state.tmp'); fs.writeFileSync(tmp,JSON.stringify(state));fs.renameSync(tmp,path.join(root,'state.json'));
 }
 const timer=setInterval(async()=>{if(busy)return;busy=true;try{
  const control=JSON.parse(fs.readFileSync(path.join(root,'control.json'),'utf8'));
  if(control.stop){
   clearInterval(timer);
   if(vscode.window.activeTextEditor?.document===document){
    await document.save();
    fs.writeFileSync(path.join(root,'closed.json'),JSON.stringify({requested:true}));
    await vscode.commands.executeCommand('workbench.action.closeWindow');
   }
   return;
  }
  if(control.nonce!==nonce && ready()){
   const edit=new vscode.WorkspaceEdit();
   edit.replace(document.uri,new vscode.Range(document.positionAt(0),document.positionAt(document.getText().length)),control.seed||'');
   if(await vscode.workspace.applyEdit(edit)){
    const end=document.positionAt(document.getText().length);
    vscode.window.activeTextEditor.selection=new vscode.Selection(end,end);nonce=control.nonce;
   }
  }
  publish();
 }finally{busy=false}},10);
 context.subscriptions.push({dispose:()=>clearInterval(timer)},
  vscode.workspace.onDidChangeTextDocument(e=>{if(e.document===document){record('text');publish()}}),
  vscode.window.onDidChangeTextEditorSelection(e=>{if(e.textEditor.document===document)record('selection')}),
  vscode.window.onDidChangeActiveTextEditor(publish),vscode.window.onDidChangeWindowState(publish));
 publish();
};
'''


class Editor:
    def __init__(self, surface, scratch, driver, wait_for, session, textedit_binary, existing_cursor=False):
        self.child = self.server = None
        self.scratch = scratch
        self.pid = None
        self.window = None
        self.short_profile = None
        self.shared_cursor = surface == 'cursor' and existing_cursor
        self.log = (scratch/'editor.log').open('w')
        try:
            if surface == 'textedit':
                self.child = subprocess.Popen([str(textedit_binary), str(scratch)], stdout=self.log, stderr=self.log)
                state = wait_for(scratch/'state.json', lambda s: s.get('ready'), timeout=25)
                self.pid = state['app_pid']
                self.window = dict(pid=self.pid, window_id=state['window'])
                return
            if surface == 'chrome':
                root = scratch
                class Handler(BaseHTTPRequestHandler):
                    def log_message(self, *args): pass
                    def do_GET(self):
                        if self.path == '/control':
                            data = (root/'control.json').read_bytes()
                        elif self.path == '/': data = HTML.encode()
                        else: self.send_error(404); return
                        self.send_response(200); self.send_header('Content-Length', str(len(data)))
                        self.end_headers(); self.wfile.write(data)
                    def do_POST(self):
                        if self.path != '/state': self.send_error(404); return
                        state = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                        state['pid'] = os.getpid()
                        # A delayed HTTP worker must not overwrite a newer state.
                        with self.server.state_lock:
                            if state['serial'] > self.server.serial:
                                self.server.serial = state['serial']; probe.write_json(root/'state.json', state)
                        self.send_response(204); self.end_headers()
                self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
                self.server.serial = 0; self.server.state_lock = threading.Lock()
                threading.Thread(target=self.server.serve_forever, daemon=True).start()
                command = ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
                           '--user-data-dir='+str(scratch/'profile'), '--no-first-run', '--no-default-browser-check',
                           '--disable-sync', '--disable-background-networking', '--disable-extensions',
                           '--app=http://127.0.0.1:'+str(self.server.server_port)+'/']
            elif surface == 'cursor':
                cursor_file = scratch/f'Upyr-probe-{scratch.name}.txt'
                cursor_file.write_text('')
                (scratch/'extension.js').write_text(EXTENSION)
                if os.environ.get('UPYR_PROBE_TRACE') == '1':
                    (scratch/'trace-enabled').touch()
                probe.write_json(scratch/'package.json', dict(name='upyr-owned-probe',version='0.0.1',publisher='upyr-local',
                    engines={'vscode':'^1.90.0'},main='./extension.js',activationEvents=['onStartupFinished']))
                settings = scratch/'profile/User'; settings.mkdir(parents=True)
                # Electron's Unix socket has a 103-byte path limit.
                self.short_profile = Path(tempfile.mkdtemp(prefix='upyr-cursor-'))
                (self.short_profile/'p').symlink_to(scratch/'profile', target_is_directory=True)
                probe.write_json(settings/'settings.json', {'workbench.startupEditor':'none','security.workspace.trust.enabled':False,
                    'extensions.autoUpdate':False,'update.mode':'none','telemetry.telemetryLevel':'off',
                    'editor.quickSuggestions':False,'editor.suggestOnTriggerCharacters':False,'editor.inlineSuggest.enabled':False})
                command = ['/Applications/Cursor.app/Contents/MacOS/Cursor','--new-window',
                           '--user-data-dir='+str(self.short_profile/'p'),'--extensions-dir='+str(scratch/'extensions'),
                           '--extensionDevelopmentPath='+str(scratch),'--skip-welcome','--skip-release-notes',
                           str(cursor_file)]
                if self.shared_cursor:
                    command = ['/Applications/Cursor.app/Contents/Resources/app/bin/cursor',
                               '--new-window','--extensionDevelopmentPath='+str(scratch),str(cursor_file)]
            else: raise ValueError(surface)
            self.child = subprocess.Popen(command, stdout=self.log, stderr=self.log)
            self.pid = self.child.pid
            deadline = time.monotonic()+35
            while time.monotonic()<deadline:
                if self.child.poll() is not None and not self.shared_cursor:
                    raise RuntimeError(f'Owned {surface} exited: see {scratch / "editor.log"}')
                windows = [w for w in driver('list_windows')['windows']
                           if (w.get('app_name')=='Cursor' and cursor_file.name in w.get('title','')
                               if self.shared_cursor else w['pid']==self.pid)
                           and w.get('bounds',{}).get('height',0)>100]
                if len(windows)==1:
                    self.pid = windows[0]['pid']
                    self.window = dict(pid=self.pid,window_id=windows[0]['window_id'])
                    driver('get_window_state',**self.window,session=session,include_screenshot=False)
                    driver('bring_to_front',**self.window)
                    break
                time.sleep(.1)
            if not self.window: raise RuntimeError('Owned editor window did not appear')
            wait_for(scratch/'state.json',lambda s:s.get('ready'),timeout=30)
        except BaseException:
            self.close()
            raise

    def close(self):
        probe.write_json(self.scratch/'control.json',dict(nonce='stop',stop=True))
        if not self.pid and self.child and (self.scratch/'owner.json').exists():
            owner = json.loads((self.scratch/'owner.json').read_text())
            if owner.get('fixture_pid') == self.child.pid:
                self.pid = owner['app_pid']
        if self.shared_cursor:
            # The development extension closes only its own scratch editor window.
            deadline = time.monotonic()+5
            while time.monotonic()<deadline and not (self.scratch/'closed.json').exists():
                time.sleep(.05)
        if self.child and self.child.poll() is None and not self.shared_cursor:
            if self.pid and self.pid != self.child.pid:
                # TextEdit was launched as a fresh instance by the Swift fixture.
                try: os.kill(self.pid,15)
                except ProcessLookupError: pass
            self.child.terminate()
            try: self.child.wait(timeout=5)
            except subprocess.TimeoutExpired: self.child.kill(); self.child.wait(timeout=3)
        if self.server: self.server.shutdown(); self.server.server_close()
        if self.short_profile:
            (self.short_profile/'p').unlink()
            self.short_profile.rmdir()
            self.short_profile = None
        self.log.close()
