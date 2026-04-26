import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import App from "./App";
import { WorkbenchPage } from "./pages/Workbench";
import { ConnectionsPage } from "./pages/Connections";
import { TablesPage } from "./pages/Tables";
import { TableDetailPage } from "./pages/TableDetail";
import { ToolsPage } from "./pages/Tools";
import { MigrationPage } from "./pages/Migration";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<App />}>
          <Route index element={<Navigate to="/workbench" replace />} />
          <Route path="workbench" element={<WorkbenchPage />} />
          <Route path="connections" element={<ConnectionsPage />} />
          <Route path="tables" element={<TablesPage />} />
          <Route path="tables/:name" element={<TableDetailPage />} />
          <Route path="tools" element={<ToolsPage />} />
          <Route path="migration" element={<MigrationPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  </React.StrictMode>,
);
