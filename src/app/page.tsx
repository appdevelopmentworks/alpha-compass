'use client';

import { useState } from 'react';
import HomeView from '@/views/HomeView';
import ConnectionView from '@/views/ConnectionView';
import UsMarketView from '@/views/UsMarketView';
import JapanMarketView from '@/views/JapanMarketView';
import DisclosuresView from '@/views/DisclosuresView';
import AlertsView from '@/views/AlertsView';
import CrossMarketView from '@/views/CrossMarketView';
import SettingsView from '@/views/SettingsView';

type Tab =
  | 'home'
  | 'jp'
  | 'us'
  | 'cross'
  | 'disc'
  | 'alerts'
  | 'settings'
  | 'connection';

const TABS: { id: Tab; label: string }[] = [
  { id: 'home', label: 'ホーム' },
  { id: 'jp', label: '日本市場' },
  { id: 'us', label: '米国市場' },
  { id: 'cross', label: 'クロスマーケット' },
  { id: 'disc', label: '開示 / ニュース' },
  { id: 'alerts', label: 'アラート' },
  { id: 'settings', label: '設定' },
  { id: 'connection', label: '疎通 / DB' },
];

export default function Home() {
  const [tab, setTab] = useState<Tab>('home');

  return (
    <main className="app">
      <header className="app__header">
        <span className="app__title">alpha-compass</span>
        <span className="app__subtitle">投資情報統合モニター</span>
      </header>

      <nav className="nav">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`nav__item ${tab === t.id ? 'nav__item--active' : ''}`}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {tab === 'home' && <HomeView />}
      {tab === 'jp' && <JapanMarketView />}
      {tab === 'us' && <UsMarketView />}
      {tab === 'cross' && <CrossMarketView />}
      {tab === 'disc' && <DisclosuresView />}
      {tab === 'alerts' && <AlertsView />}
      {tab === 'settings' && <SettingsView />}
      {tab === 'connection' && <ConnectionView />}
    </main>
  );
}
