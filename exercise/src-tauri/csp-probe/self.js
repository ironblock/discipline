fetch('http://127.0.0.1:7988/bundle-script-ran', { mode: 'no-cors' });
const injected = document.createElement('script');
injected.textContent = "fetch('http://127.0.0.1:7988/injected-script-ran', { mode: 'no-cors' })";
document.body.appendChild(injected);
try { eval("fetch('http://127.0.0.1:7988/eval-ran', { mode: 'no-cors' })"); } catch (e) { fetch('http://127.0.0.1:7988/eval-refused', { mode: 'no-cors' }); }
