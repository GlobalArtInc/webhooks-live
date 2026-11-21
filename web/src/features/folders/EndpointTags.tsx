import { useTranslation } from 'react-i18next';
import { ChipInput } from '../../components/ChipInput/ChipInput';
import { isValidTag, MAX_TAGS_PER_ENDPOINT, normalizeTag } from './localOrganize';

export interface EndpointTagsProps {
  name: string;
  tags: readonly string[];
  suggestions: readonly string[];
  readOnly: boolean;
  onChange: (tags: string[]) => void;
}

export function EndpointTags({ name, tags, suggestions, readOnly, onChange }: EndpointTagsProps) {
  const { t } = useTranslation();
  return (
    <ChipInput
      label={t('wl.rail.tags.labelFor', { name })}
      values={tags}
      onChange={onChange}
      normalize={normalizeTag}
      validate={(value) => (isValidTag(value) ? null : t('wl.rail.tags.invalid'))}
      suggestions={suggestions}
      maxItems={MAX_TAGS_PER_ENDPOINT}
      readOnly={readOnly}
    />
  );
}
