import { S3Client, PutObjectCommand, HeadObjectCommand } from "@aws-sdk/client-s3";

export function frameKey(repoKey: string, hash: string, pose: string): string {
  return `${repoKey}/${hash}/${pose}.png`;
}

export function sheetKey(repoKey: string, hash: string): string {
  return `${repoKey}/${hash}/_sheet.png`;
}

export function frameUri(bucket: string, key: string): string {
  return `s3://${bucket}/${key}`;
}

export interface FrameStore {
  put(key: string, body: Buffer, contentType: string): Promise<string>;
  exists(key: string): Promise<boolean>;
}

/** S3 client surface we actually use — keeps the fake in tests tiny. */
type S3Like = Pick<S3Client, "send">;

export class SeaweedFsStore implements FrameStore {
  constructor(
    private readonly s3: S3Like,
    private readonly bucket: string,
  ) {}

  async put(key: string, body: Buffer, contentType: string): Promise<string> {
    await this.s3.send(
      new PutObjectCommand({ Bucket: this.bucket, Key: key, Body: body, ContentType: contentType }) as never,
    );
    return frameUri(this.bucket, key);
  }

  async exists(key: string): Promise<boolean> {
    try {
      await this.s3.send(new HeadObjectCommand({ Bucket: this.bucket, Key: key }) as never);
      return true;
    } catch {
      return false;
    }
  }
}

/** Build a path-style S3 client for SeaweedFS from env-injected config. */
export function makeSeaweedClient(cfg: {
  endpoint: string;
  region: string;
  accessKeyId: string;
  secretAccessKey: string;
}): S3Client {
  return new S3Client({
    endpoint: cfg.endpoint,
    region: cfg.region,
    forcePathStyle: true,
    credentials: { accessKeyId: cfg.accessKeyId, secretAccessKey: cfg.secretAccessKey },
  });
}
