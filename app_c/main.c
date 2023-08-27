typedef struct
{
    unsigned char version;
    unsigned long long start_time;
    void (*log)(const char *, unsigned int);
    unsigned long long pid;

} Context;

int _start(Context *ctx)
{
    char text[] = "[pid:_] Hello from C!";
    text[5] = '0' + ctx->pid;
    ctx->log(text, sizeof(text) - 1);
    return 0;
}