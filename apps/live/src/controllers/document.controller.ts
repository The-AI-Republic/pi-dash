/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { Request, Response } from "express";
import * as Y from "yjs";
import { z } from "zod";
// helpers
import { Controller, Post } from "@pi-dash/decorators";
import { convertBase64StringToBinaryData, convertHTMLDocumentToAllFormats } from "@pi-dash/editor";
// logger
import { logger } from "@pi-dash/logger";
import type { TConvertDocumentRequestBody } from "@/types";

// Define the schema with more robust validation
const convertDocumentSchema = z.object({
  description_html: z
    .string()
    .min(1, "HTML content cannot be empty")
    .refine((html) => html.trim().length > 0, "HTML content cannot be just whitespace")
    .refine((html) => html.includes("<") && html.includes(">"), "Content must be valid HTML"),
  variant: z.enum(["rich", "document"]),
  // Optional current Yjs state (base64). When present, the new content is applied as an in-place
  // diff onto it so browsers holding a cached copy of the doc merge without duplicating content.
  description_binary: z
    .string()
    .nullish()
    .refine((value) => value == null || value === "" || isValidBase64YjsUpdate(value), {
      message: "description_binary must be a base64-encoded Yjs update",
    }),
  title: z.string().nullish(),
});

const BASE64_REGEX = /^[A-Za-z0-9+/]*={0,2}$/;

/**
 * Strictly validate a base64 string (Buffer.from silently ignores invalid characters) and check
 * that it decodes to something Yjs can apply.
 */
function isValidBase64YjsUpdate(value: string): boolean {
  if (value.length % 4 !== 0 || !BASE64_REGEX.test(value)) return false;
  try {
    Y.applyUpdate(new Y.Doc(), convertBase64StringToBinaryData(value));
    return true;
  } catch {
    return false;
  }
}

@Controller("/convert-document")
export class DocumentController {
  @Post("/")
  async convertDocument(req: Request, res: Response) {
    try {
      // Validate request body
      const validatedData = convertDocumentSchema.parse(req.body as TConvertDocumentRequestBody);
      const { description_html, variant, title } = validatedData;
      const base_binary = validatedData.description_binary
        ? new Uint8Array(convertBase64StringToBinaryData(validatedData.description_binary))
        : undefined;

      // Process document conversion
      const result = convertHTMLDocumentToAllFormats({
        document_html: description_html,
        variant,
        base_binary,
        title,
      });

      // Return successful response
      res.status(200).json({
        description_json: result.description_json,
        description_binary: result.description_binary,
        description_html: result.description_html,
      });
    } catch (error) {
      if (error instanceof z.ZodError) {
        const validationErrors = error.errors.map((err) => ({
          path: err.path.join("."),
          message: err.message,
        }));
        logger.error("DOCUMENT_CONTROLLER: Validation error", {
          validationErrors,
        });
        return res.status(400).json({
          message: `Validation error`,
          context: {
            validationErrors,
          },
        });
      } else {
        logger.error("DOCUMENT_CONTROLLER: Internal server error", error);
        return res.status(500).json({
          message: `Internal server error.`,
        });
      }
    }
  }
}
