import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { AppShell } from './components/layout/AppShell';
import { Home } from './pages/Home';
import { Chat } from './pages/Chat';
import { Image } from './pages/Image';
import { Voice } from './pages/Voice';
import { Models } from './pages/Models';
import { System } from './pages/System';
import { Settings } from './pages/Settings';
import { useAccentTheme, AccentThemeProvider } from './hooks/useAccentTheme';
import { useTheme, ThemeProvider } from './hooks/useTheme';
import { useDensity, DensityProvider } from './hooks/useDensity';

function AppContent() {
  useAccentTheme();
  useDensity();
  const { isDark } = useTheme();

  return (
    <div className={isDark ? 'dark' : ''}>
      <BrowserRouter>
        <Routes>
          <Route element={<AppShell />}>
            <Route path="/" element={<Home />} />
            <Route path="/chat" element={<Chat />} />
            <Route path="/image" element={<Image />} />
            <Route path="/voice" element={<Voice />} />
            <Route path="/models" element={<Models />} />
            <Route path="/system" element={<System />} />
            <Route path="/settings" element={<Settings />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </div>
  );
}

function App() {
  return (
    <ThemeProvider>
      <AccentThemeProvider>
        <DensityProvider>
          <AppContent />
        </DensityProvider>
      </AccentThemeProvider>
    </ThemeProvider>
  );
}

export default App;

