import { create } from 'zustand';

interface MarketState {
  installedPlugins: string[];
}

export const useMarketStore = create<MarketState>(() => ({
  installedPlugins: [],
}));
