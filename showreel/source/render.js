// usage: node shot.js <t1,t2,...> | node shot.js all
const puppeteer=require('puppeteer-core'), path=require('path'), fs=require('fs');
(async()=>{
  const b=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:'new',args:['--allow-file-access-from-files','--force-device-scale-factor=1','--hide-scrollbars','--font-render-hinting=none']});
  const p=await b.newPage(); await p.setViewport({width:1920,height:1080,deviceScaleFactor:1});
  p.on('console',m=>console.log('page:',m.text())); p.on('pageerror',e=>console.log('ERR',e.message));
  await p.goto('file://'+path.resolve(__dirname,'reel.html')); await p.waitForFunction('window.READY===true',{timeout:60000});
  const arg=process.argv[2]; const all=arg==='all'; const out=path.resolve(__dirname,all?'frames':(process.argv[3]||'stills')); fs.mkdirSync(out,{recursive:true});
  const ts=all?Array.from({length:900},(_,i)=>i/60):arg.split(',').map(Number);
  for(let i=0;i<ts.length;i++){ await p.evaluate(t=>{seek(t);return new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))},ts[i]);
    await p.screenshot({path:path.join(out,all?`f${String(i).padStart(4,'0')}.jpg`:`t${ts[i].toFixed(2)}.jpg`),type:'jpeg',quality:all?95:80}); if(all&&i%100===0)console.log(i); }
  await b.close();
})();
