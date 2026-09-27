import React, { createContext, useContext, useState, useEffect, useCallback } from 'react';

export type Density = 'compact' | 'medium' | 'spacious';

export function applyDensity(density: Density): void {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.density = density;
}

// Immediately apply initial density on script load
if (typeof window !== 'undefined') {
  const saved = localStorage.getItem('gabriel_ui_density') as Density;
  if (saved && ['compact', 'medium', 'spacious'].includes(saved)) {
    applyDensity(saved);
  } else {
    applyDensity('medium');
  }
}

export interface DensityContextType {
  density: Density;
  setDensity: (density: Density) => void;
}

const DensityContext = createContext<DensityContextType | undefined>(undefined);

export const DensityProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [density, setDensityState] = useState<Density>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('gabriel_ui_density') as Density;
      if (saved && ['compact', 'medium', 'spacious'].includes(saved)) {
        return saved;
      }
    }
    return 'medium';
  });

  useEffect(() => {
    applyDensity(density);
    if (typeof window !== 'undefined') {
      localStorage.setItem('gabriel_ui_density', density);
    }
  }, [density]);

  const setDensity = useCallback((d: Density) => {
    applyDensity(d);
    setDensityState(d);
  }, []);

  return (
    <DensityContext.Provider value={{ density, setDensity }}>
      {children}
    </DensityContext.Provider>
  );
};

export function useDensity(): DensityContextType {
  const context = useContext(DensityContext);
  if (!context) {
    const saved = typeof window !== 'undefined'
      ? (localStorage.getItem('gabriel_ui_density') as Density) || 'medium'
      : 'medium';
    const valid = ['compact', 'medium', 'spacious'].includes(saved) ? saved : 'medium';
    return {
      density: valid,
      setDensity: (d: Density) => applyDensity(d),
    };
  }
  return context;
}
