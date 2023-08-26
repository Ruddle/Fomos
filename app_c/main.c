typedef struct
{
    unsigned char version;
    unsigned long long start_time;
    void (*log)(const char *, unsigned int);
    unsigned long long pid;

} Context;

int _start(Context *ctx)
{
    char hwText[] = "[pid:_] Hello from C!";
    hwText[5] = '0' + ctx->pid;
    ctx->log(hwText, sizeof(hwText) - 1);
    return 0;
}