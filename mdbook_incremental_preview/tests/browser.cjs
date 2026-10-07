const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const {spawn} = require('node:child_process');
const {pathToFileURL} = require('node:url');
const {chromium} = require('playwright');
(async () => {
 const root = await fs.mkdtemp(path.join(os.tmpdir(), 'mdbook-search-browser-'));
 await fs.mkdir(path.join(root,'src'));
 await fs.writeFile(path.join(root,'book.toml'), '[book]\ntitle="search"\n');
 await fs.writeFile(path.join(root,'src/SUMMARY.md'), '# Summary\n\n- [Chapter](<chapter #?.md>)\n');
 const text = '# heading\n\nparagraph\n\n- list item\n\n```rust\nlet x = 1;\n```\n\n    indented code\n\n---\n';
 const chapter = path.join(root,'src/chapter #?.md');
 const chapterUri = pathToFileURL(chapter).href;
 const chapterUrl = 'http://127.0.0.1:33029/chapter%20%23%3F.html';
 await fs.writeFile(chapter,text.replace('paragraph','saved disk paragraph'));
 const child = spawn(process.env.MDBOOK_LS_BINARY || path.resolve(__dirname,'../../target/debug/mdbook-ls'),[],{stdio:['pipe','pipe','pipe']});
 child.stderr.on('data', x => process.stderr.write(x));
 let buffer=Buffer.alloc(0), id=0; const pending=new Map(), events=[];
 child.stdout.on('data', bytes => {buffer=Buffer.concat([buffer,bytes]);for(;;){const end=buffer.indexOf('\r\n\r\n');if(end<0)break;const n=Number(buffer.subarray(0,end).toString().match(/Content-Length: (\d+)/i)[1]);if(buffer.length<end+4+n)break;const msg=JSON.parse(buffer.subarray(end+4,end+4+n));buffer=buffer.subarray(end+4+n);if(msg.id!==undefined){pending.get(msg.id)?.(msg);pending.delete(msg.id);}else events.push(msg);}});
 const send = (method, params, request=true) => {const msg={jsonrpc:'2.0',method,params};if(request)msg.id=++id;const data=JSON.stringify(msg);child.stdin.write(`Content-Length: ${Buffer.byteLength(data)}\r\n\r\n${data}`);return request?new Promise(resolve=>pending.set(msg.id,resolve)):undefined;};
 let browser;
 try {
  await send('initialize',{rootUri:pathToFileURL(root).href,capabilities:{}});send('initialized',{},false);
  send('textDocument/didOpen',{textDocument:{uri:chapterUri,languageId:'markdown',version:1,text}},false);
  await send('workspace/executeCommand',{command:'open_preview',arguments:['127.0.0.1:33029']});
  browser=await chromium.launch({executablePath:process.env.CHROME_BINARY,headless:true,args:['--no-sandbox']});
  const page=await browser.newPage();
  for(let n=0;n<100;n++){try{await page.goto(chapterUrl);break;}catch(e){await new Promise(r=>setTimeout(r,100));}}
  await page.waitForSelector('main h1');
  await page.waitForFunction(()=>document.querySelector('main > p')?.textContent==='paragraph');
  await page.waitForTimeout(500);
  for(const [selector,line] of [['main h1',0],['main > p',2],['main li',4],['main pre',6],['main pre:nth-of-type(2)',10],['main hr',12]]){
   const initial=events.length;await page.locator(selector).first().click({modifiers:['Control']});
   await page.waitForTimeout(100);const event=events.slice(initial).find(x=>x.method==='mdbook/reverseSearch');assert(event,selector);assert.equal(event.params.position.line,line,selector);
  }
  await send('workspace/executeCommand',{command:'forward_search',arguments:[chapter,{line:10,character:0}]});
  await page.waitForFunction(()=>location.hash==='#mdbook-source-line=10');
  assert.equal(await page.locator('main pre:nth-of-type(2)').innerText(),'indented code\n');
  const unsaved='\n\n'+text.replace('paragraph','UNSAVED paragraph');
  send('textDocument/didChange',{textDocument:{uri:chapterUri,version:2},contentChanges:[{text:unsaved}]},false);
  await page.waitForFunction(()=>document.querySelector('main').textContent.includes('UNSAVED'));
  const initial=events.length;await page.locator('main > p').first().click({modifiers:['Control']});await page.waitForTimeout(100);
  assert.equal(events.slice(initial).find(x=>x.method==='mdbook/reverseSearch').params.position.line,4);
  await page.reload();
  await page.waitForFunction(()=>document.querySelector('main').textContent.includes('UNSAVED'));
  for (const alias of ['/', '/index.html']) {
   await page.goto('http://127.0.0.1:33029'+alias);
   await page.waitForFunction(()=>document.querySelector('main').textContent.includes('UNSAVED'));
  }
  await fs.writeFile(path.join(root,'book.toml'),'[book]\ntitle="rebuilt search"\n');
  await page.waitForTimeout(700);
  await page.reload();
  await page.waitForFunction(()=>document.querySelector('main').textContent.includes('UNSAVED'));
  send('textDocument/didClose',{textDocument:{uri:chapterUri}},false);
  await page.waitForFunction(()=>document.querySelector('main').textContent.includes('saved disk paragraph'));
  await page.reload();
  await page.waitForFunction(()=>document.querySelector('main').textContent.includes('saved disk paragraph'));
  assert(!await page.locator('main').innerText().then(text=>text.includes('UNSAVED')));
  console.log('browser DOM reverse/forward, unsaved replay and discard passed');
 }finally{await browser?.close();child.kill();await fs.rm(root,{recursive:true,force:true});}
})().catch(e=>{console.error(e);process.exitCode=1;});
