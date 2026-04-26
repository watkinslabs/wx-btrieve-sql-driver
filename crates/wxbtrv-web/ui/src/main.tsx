import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import App from "./App";
import { ConnectionPage } from "./pages/Connection";
import { TablesPage } from "./pages/Tables";
import { TableDetailPage } from "./pages/TableDetail";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<App />}>
          <Route index element={<Navigate to="/connection" replace />} />
          <Route path="connection" element={<ConnectionPage />} />
          <Route path="tables" element={<TablesPage />} />
          <Route path="tables/:name" element={<TableDetailPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  </React.StrictMode>,
);
