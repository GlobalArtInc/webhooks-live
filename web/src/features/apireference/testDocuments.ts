import { parseOpenApiDocument, type OpenApiDocument } from './document';

export function miniDocument(components: Record<string, unknown>): OpenApiDocument {
  return parseOpenApiDocument({ openapi: '3.1.0', paths: {}, components: { schemas: components } });
}

export function schemaRef(name: string): { $ref: string } {
  return { $ref: `#/components/schemas/${name}` };
}
