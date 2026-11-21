import { MyEndpointsPage } from '../../features/folders/MyEndpointsPage';
import type { EndpointsSearch } from '../../features/folders/search';

export interface EndpointsScreenProps {
  search: EndpointsSearch;
}

export function EndpointsScreen({ search }: EndpointsScreenProps) {
  return <MyEndpointsPage search={search} />;
}
