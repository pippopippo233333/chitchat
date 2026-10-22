import { mount } from 'svelte';
import App from './App.svelte';
import './style.css';
import { applyTheme, loadTheme } from './lib/theme';

mount(App, { target: document.getElementById('app')! });
applyTheme(loadTheme());
