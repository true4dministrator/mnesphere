import { mount } from 'svelte';
import App from './App.svelte';
// KaTeX 自带样式与 woff2 字体，必须全局引入 —— 只渲染公式的地方引会漏掉字体，
// 结果是公式排得对但字形退化成系统衬线体（那种「看着像但就是不对」的错）。
// Vite 会把字体一起打进 dist/assets 并在 CSS 里改写成相对路径。
import 'katex/dist/katex.min.css';
import './app.css';

const app = mount(App, { target: document.getElementById('app')! });

export default app;
