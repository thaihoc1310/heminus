# pnpm dev, then: GDK_BACKEND=x11 xvfb-run -a python3 scripts/terminal-smoke-webkit.py scripts/terminal-smoke-webkit.js
# Runs the real TerminalPane in WebKitGTK (the Linux webview) and prints PASS/FAIL lines.
import gi,sys; gi.require_version('WebKit2','4.1'); gi.require_version('Gtk','3.0')
from gi.repository import WebKit2, Gtk, GLib
w=Gtk.OffscreenWindow(); v=WebKit2.WebView(); w.set_default_size(1000,700); w.add(v); w.show_all()
body=open(sys.argv[1]).read()
def done(view,res,_):
    try: print(view.call_async_javascript_function_finish(res).to_string())
    except Exception as e: print("ERROR",e)
    Gtk.main_quit()
v.connect('load-changed',lambda view,ev: ev==WebKit2.LoadEvent.FINISHED and view.call_async_javascript_function(body,-1,None,None,None,None,done,None))
v.load_uri("http://localhost:1420/scripts/terminal-smoke.html")
GLib.timeout_add_seconds(120,lambda:(print("TIMEOUT"),Gtk.main_quit()))
Gtk.main()
