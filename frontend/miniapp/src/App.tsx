import { Navigate, Route, Routes } from 'react-router-dom';

import { PredictionsPage } from './pages/PredictionsPage';
import { RankingPage } from './pages/RankingPage';

/**
 * Top-level router. The Mini App opens at one of the configured launch URLs;
 * we map those to local routes here.
 */
export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Navigate to="/predictions" replace />} />
      <Route path="/predictions" element={<PredictionsPage />} />
      <Route path="/ranking" element={<RankingPage />} />
    </Routes>
  );
}
