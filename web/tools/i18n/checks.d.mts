export interface CheckReport {
  errors: string[];
  warnings: string[];
}

export function checkTree(root: string): CheckReport;
