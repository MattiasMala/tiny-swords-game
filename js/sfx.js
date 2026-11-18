'use strict';
// Effetti sonori sintetizzati con WebAudio (il pacchetto non contiene audio).

const Sfx = {
  ctx: null,
  enabled: true,
  last: {},

  // I browser consentono l'audio solo dopo un gesto dell'utente.
  unlock() {
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (AC) this.ctx = new AC();
    }
    if (this.ctx && this.ctx.state === 'suspended') this.ctx.resume();
  },

  tone(freq, dur, type, vol, slideTo) {
    const ac = this.ctx, t = ac.currentTime;
    const o = ac.createOscillator(), g = ac.createGain();
    o.type = type;
    o.frequency.setValueAtTime(freq, t);
    if (slideTo) o.frequency.exponentialRampToValueAtTime(slideTo, t + dur);
    g.gain.setValueAtTime(vol, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(ac.destination);
    o.start(t); o.stop(t + dur);
  },

  noise(dur, vol, cutoff) {
    const ac = this.ctx, t = ac.currentTime;
    const len = Math.max(1, Math.floor(ac.sampleRate * dur));
    const buf = ac.createBuffer(1, len, ac.sampleRate), d = buf.getChannelData(0);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / len);
    const src = ac.createBufferSource(), f = ac.createBiquadFilter(), g = ac.createGain();
    src.buffer = buf;
    f.type = 'lowpass'; f.frequency.value = cutoff;
    g.gain.value = vol;
    src.connect(f).connect(g).connect(ac.destination);
    src.start(t);
  },

  play(name, vol = 1) {
    if (!this.enabled || !this.ctx || this.ctx.state !== 'running' || vol <= 0.02) return;
    const now = performance.now();
    if (now - (this.last[name] || 0) < 45) return;
    this.last[name] = now;
    const v = 0.07 * vol;
    switch (name) {
      case 'hit': this.noise(0.09, v * 2.2, 1800); this.tone(180, 0.08, 'square', v * 0.6, 90); break;
      case 'block': this.tone(900, 0.07, 'square', v * 0.6, 500); this.noise(0.05, v * 1.5, 5000); break;
      case 'shoot': this.tone(700, 0.12, 'triangle', v, 250); break;
      case 'chop': this.noise(0.06, v * 1.6, 900); break;
      case 'coin': this.tone(990, 0.07, 'square', v * 0.7); this.tone(1480, 0.12, 'square', v * 0.6); break;
      case 'heal': this.tone(520, 0.25, 'sine', v * 1.4, 1040); break;
      case 'build': this.tone(330, 0.1, 'triangle', v * 1.4); this.tone(495, 0.18, 'triangle', v * 1.2); break;
      case 'death': this.tone(260, 0.22, 'sawtooth', v * 0.7, 70); break;
      case 'boom': this.noise(0.5, v * 3.5, 500); this.tone(90, 0.4, 'sine', v * 2.5, 40); break;
      case 'click': this.tone(600, 0.05, 'square', v * 0.7); break;
      case 'select': this.tone(520, 0.04, 'triangle', v * 0.9); break;
      case 'ack': this.tone(440, 0.05, 'triangle', v); this.tone(660, 0.07, 'triangle', v * 0.8); break;
      case 'research': [660, 880, 1100].forEach((f, i) => setTimeout(() => this.tone(f, 0.16, 'triangle', v * 1.3), i * 90)); break;
      case 'age': [392, 523, 659, 784, 1047].forEach((f, i) => setTimeout(() => this.tone(f, 0.35, 'sawtooth', v * 1.1), i * 140)); break;
      case 'deny': this.tone(160, 0.14, 'square', v * 0.8, 110); break;
      case 'horn': this.tone(196, 0.5, 'sawtooth', v * 0.9); this.tone(294, 0.6, 'sawtooth', v * 0.6); break;
      case 'win': [523, 659, 784, 1047].forEach((f, i) => setTimeout(() => this.tone(f, 0.3, 'triangle', v * 1.6), i * 130)); break;
      case 'lose': [392, 330, 262, 196].forEach((f, i) => setTimeout(() => this.tone(f, 0.4, 'sawtooth', v), i * 180)); break;
    }
  },

  // Suono in un punto del mondo: lo sente, attenuato con la distanza, ogni giocatore che lo ha vicino.
  at(name, x, y) { emit(['snd', name, Math.round(x), Math.round(y)]); },
  hear(name, x, y) {
    if (!this.enabled || !this.ctx || G.demo) return;
    const d = Math.hypot(x - (G.cam.x + G.viewW / 2), y - (G.cam.y + G.viewH / 2));
    this.play(name, 1 - d / (G.viewW * 0.75));
  },
};
