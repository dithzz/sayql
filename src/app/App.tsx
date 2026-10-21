import { AlertOctagon } from "lucide-react";
import { EmptyState, Spinner } from "../components/ui";
import { ThemeProvider } from "../features/theme";
import { useBootstrap } from "./useBootstrap";
import { Workbench } from "./Workbench";
import styles from "./App.module.css";

export function App() {
  const { data, error } = useBootstrap();
  return (
    <ThemeProvider>
      {data ? (
        <Workbench boot={data} />
      ) : (
        <div className={styles.splash}>
          {error ? (
            <EmptyState icon={AlertOctagon} title="SayQL couldn't start">
              {error}
            </EmptyState>
          ) : (
            <Spinner size={20} />
          )}
        </div>
      )}
    </ThemeProvider>
  );
}
