import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'alpha-compass',
  description: '日本市場ファースト × 米国強化の投資情報統合モニター',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="ja">
      <body>{children}</body>
    </html>
  );
}
