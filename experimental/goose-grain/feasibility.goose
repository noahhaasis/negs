import std;
import os;
import math;
struct Job { n: i64 }
struct Answer { n: i64 }
thread_fn worker() { let j=qget<Job>(); qput(Answer { j.n+1 }); }
fn main() {
    let t=clock_ns();
    var a: f64[>..]=[];
    a.push(0.10000000000000002); a.push(-1.2345678901234567);
    let b=copy(a);
    let h=thread_spawn(worker);
    qput(Job { 41 });
    let result=qget<Answer>(); let n=result.n;
    thread_wait(h);
    assert(n==42 && a==b && clock_ns()>=t);
    print(a);
}
