#include <stdio.h>
int main(void)
{
    for (int i = 0; i < 5; i++)
    {
        if (i % 2 == 0)
        {
            printf("%i is even\n", i);
        }
        else
        {
            printf("%i is odd\n", i);
        }
    }
    return 0;
}
