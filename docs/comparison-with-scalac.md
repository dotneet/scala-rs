## Comparison with scalac 2.13

The honest diff.

- **Scale**: a very small part of nsc. It does not meet the language
  specification.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` /
  `Predef$` (`println` / `assert` / `require` / `???` / `identity` / `locally` /
  `implicitly`) / `any2stringadd` / `->` from `ArrowAssoc` / `intWrapper` /
  `RichInt` (`abs` / `max` / `min` / `to` / `until`) / `longWrapper` /
  `RichLong` (`abs` / `max` / `min` / `to` / `until` → a real
  `NumericRange[Long]`) / `doubleWrapper` / `RichDouble` (`abs` / `max` /
  `min`) / `floatWrapper` / `RichFloat` (`abs` / `max` / `min`) / `charWrapper` /
  `RichChar` (`isDigit` / `toInt` via `intValue$extension` / `to` / `until` → a
  real `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` / `max` / `min` /
  `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` / `RichShort`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Short]`) /
  `booleanWrapper` / `RichBoolean.compare` (the instance `compare(Object)`) /
  `StringOps` (`toInt$extension` / `size$extension` / `$times$extension` /
  `take$extension` / `drop$extension` / `isEmpty` via `augmentString` /
  `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` /
  `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` /
  `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` /
  `lines$extension` / `capitalize$extension` / `reverse$extension` /
  `slice$extension` / `takeRight$extension` / `dropRight$extension` /
  `contains$extension(Char)` / `head$extension` / `last$extension` /
  `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` /
  `init$extension` / `distinct$extension` / `mkString$extension`) / `WithFilter` /
  `Iterator` / `Map` / `Vector` / `IndexedSeq` (unqualified
  `IndexedSeq(1, 2)(1)`) / `Queue` (`enqueue` / `dequeue` of
  `scala.collection.immutable.Queue`) / `ArrayBuffer` (varargs `apply` / `+=` /
  `apply` / `update` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (varargs `apply` / `+=` / `apply` of
  `scala.collection.mutable.ListBuffer`) / `StringBuilder` (`new` / `+=` /
  `append` / `toString` of `scala.collection.mutable.StringBuilder`) / `HashMap`
  (companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` and right-biased
  `isLeft` / `isRight` / `map` / `flatMap` / `fold` / `getOrElse` / `orElse` /
  `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` /
  `filterOrElse` / `left`; no `Either$LeftProjection` class file is emitted) /
  `Try` (`apply` on `Try$` / `Success` / `Failure`, plus `isSuccess` /
  `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `withFilter`
  (`Try$WithFilter`) / `foreach` / `orElse` / `recover` / `recoverWith` /
  `collect` / `toOption` / `toEither` / `failed` / `transform` / `fold`) /
  `Array$` (varargs `apply` + `ClassTag`). Dual-run: `hello` / `option_for` /
  `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` /
  `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` /
  `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` /
  `try_util` / `existentials` / `existential_bounds` / `implicit_specific` /
  `lambda_lift` / `view_bounds` / `view_bounds_class` / `hk_types` / `app` /
  `delayed_init` / `implicit_inherit_local` / `partial_function` /
  `list_collect` / `string_interp` / `overloading` / `classtag` /
  `context_bounds` / `context_bounds_class` / `type_member_hk` / `refine_hk` /
  `refine_bound` / `nested_proj` / `type_member_bounds` / `assign_op` /
  `collection_converters` / `pkg_implicit_class` / `structural_update` /
  `indexedseq_queue` / `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` /
  `numeric_range` / `listbuffer` / `string_ops5` / `short_range` /
  `stringbuilder` / `string_ops6` / `long_range` / `hashmap` / `string_ops7` /
  `char_range` / `hashset` / `string_ops8` / `array_ops2` / `linkedhashmap` /
  `string_ops9` / `array_ops3` / `linkedhashset` / `string_ops10` / `array_ops4` /
  `arraydeque` / `custom_interp` / `array_ops` / `either_ops` / `either_left` /
  `either_for` / `option_x1` / `option_x2` / `try_ops` / `try_recover` /
  `try_for`. **Still intrinsic / private, or not linked**: the rest of
  `StringOps`, the rest of the numerics, the other mutable collections.
  `List.unapplySeq` is `SeqOps`'s identity in the library. The varargs `apply` of
  `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` /
  `Predef$` (`println` / `assert` / `require` / `???` / `identity` / `locally` /
  `implicitly`) / `any2stringadd` / `->` from `ArrowAssoc` / `intWrapper` /
  `RichInt` (`abs` / `max` / `min` / `to` / `until` / `toBinaryString` /
  `toHexString` / `toOctalString` / `sign`; `Range` (`withFilter` / `filter` /
  `map` / `flatMap` / `foldLeft` / `foldRight` / `sum` / `product` / `min` /
  `max` / `toList` / `toVector` / `zipWithIndex` / `take` / `drop`, …) and
  `scala.math` (`abs` / `max` / `min` / `signum` / `pow` / `sqrt` / `floor` /
  `ceil` / `round` / `random`) came along too) / `longWrapper` / `RichLong`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Long]`) /
  `doubleWrapper` / `RichDouble` (`abs` / `max` / `min`) / `floatWrapper` /
  `RichFloat` (`abs` / `max` / `min`) / `charWrapper` / `RichChar` (`isDigit` /
  `toInt` via `intValue$extension` / `to` / `until` → a real
  `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` / `max` / `min` /
  `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` / `RichShort`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Short]`) /
  `booleanWrapper` / `RichBoolean.compare` (the instance `compare(Object)`) /
  `StringOps` (`toInt$extension` / `size$extension` / `$times$extension` /
  `take$extension` / `drop$extension` / `isEmpty` via `augmentString` /
  `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` /
  `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` /
  `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` /
  `lines$extension` / `capitalize$extension` / `reverse$extension` /
  `slice$extension` / `takeRight$extension` / `dropRight$extension` /
  `contains$extension(Char)` / `head$extension` / `last$extension` /
  `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` /
  `init$extension` / `distinct$extension` / `mkString$extension`) / `WithFilter` /
  `Iterator` / `Map` / `Vector` / `IndexedSeq` (unqualified
  `IndexedSeq(1, 2)(1)`) / `Queue` (`enqueue` / `dequeue` of
  `scala.collection.immutable.Queue`) / `ArrayBuffer` (varargs `apply` / `+=` /
  `apply` / `update` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (varargs `apply` / `+=` / `apply` of `scala.collection.mutable.ListBuffer`) /
  `StringBuilder` (the bare name / `new` / every `append` overload / `+=` /
  `++=` / `insert` / `deleteCharAt` / `setLength` / `reverse` / `clear` /
  `toString` / `result` of `scala.collection.mutable.StringBuilder`) / `HashMap`
  (companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` / `isLeft` /
  `getOrElse` / `map`) / `Try` (`apply` / `map` / `getOrElse` on `Try$` /
  `Success` / `Failure`) / `Array$` (varargs `apply` + `ClassTag`). Dual-run:
  `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` /
  `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` /
  `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` /
  `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` /
  `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` /
  `view_bounds_class` / `hk_types` / `app` / `delayed_init` /
  `implicit_inherit_local` / `partial_function` / `list_collect` /
  `string_interp` / `overloading` / `classtag` / `context_bounds` /
  `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` /
  `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` /
  `pkg_implicit_class` / `structural_update` / `indexedseq_queue` /
  `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` /
  `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` /
  `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` /
  `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` /
  `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` /
  `custom_interp` / `array_ops`. **Still intrinsic / private, or not linked**:
  the rest of `StringOps`, the rest of the numerics, the other mutable
  collections. `List.unapplySeq` is `SeqOps`'s identity in the library. The
  varargs `apply` of `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` (`_1` / `_2`, plus `swap` /
  `toString`) / `NotImplementedError` / `Predef$` (`println` / `assert` /
  `require` / `???` / `identity` / `locally` / `implicitly`) / `any2stringadd` /
  `->` from `ArrowAssoc` / `intWrapper` / `RichInt` (`abs` / `max` / `min` /
  `to` / `until`) / `longWrapper` / `RichLong` (`abs` / `max` / `min` / `to` /
  `until` → a real `NumericRange[Long]`) / `doubleWrapper` / `RichDouble` (`abs` /
  `max` / `min`) / `floatWrapper` / `RichFloat` (`abs` / `max` / `min`) /
  `charWrapper` / `RichChar` (`isDigit` / `toInt` via `intValue$extension` / `to` /
  `until` → a real `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` /
  `max` / `min` / `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` /
  `RichShort` (`abs` / `max` / `min` / `to` / `until` → a real
  `NumericRange[Short]`) / `booleanWrapper` / `RichBoolean.compare` (the instance
  `compare(Object)`) / `StringOps` (`toInt$extension` / `size$extension` /
  `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via
  `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` /
  `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` /
  `padTo$extension(Int,Char)` / `linesIterator$extension` /
  `toIntOption$extension` / `stripMargin$extension` / `lines$extension` /
  `capitalize$extension` / `reverse$extension` / `slice$extension` /
  `takeRight$extension` / `dropRight$extension` / `contains$extension(Char)` /
  `head$extension` / `last$extension` / `stripLineEnd$extension` /
  `replaceAllLiterally$extension` / `tail$extension` / `init$extension` /
  `distinct$extension` / `mkString$extension`) / `WithFilter` / `Iterator` /
  `Map` (`apply` / `get` / `updated` / `+` / `foreach`, plus `getOrElse` /
  `contains` / `keys` / `values` / `keySet` / `-` / `size` / `isEmpty` /
  `nonEmpty` / `filter` / `toList` / `toSeq` / `iterator` / `mkString` / `head` /
  `foldLeft` / `withDefaultValue` / `view` / `MapView.mapValues`) / `Vector`
  (`apply` / `length` / `updated` / `:+` / `foreach`, plus `size` / `isEmpty` /
  `nonEmpty` / `head` / `map` / `filter` / `toList` / `toSeq` / `iterator` /
  `mkString` / `foldLeft`) / `IndexedSeq` (unqualified `IndexedSeq(1, 2)(1)`) /
  `Queue` (`enqueue` / `dequeue` of `scala.collection.immutable.Queue`) /
  `ArrayBuffer` (varargs `apply` / `+=` / `apply` / `update` / `length` / `size` /
  `isEmpty` / `nonEmpty` / `head` / `last` / `mkString`(0/1/3) / `foreach` /
  `map` / `filter` / `toList` / `iterator` / `clear` / `remove` / `insert` /
  `contains` / `indexOf` / `reverse` / `foldLeft` / `append` / `++=` / `-=` /
  `sortBy` / `sorted` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (the same set of members of `scala.collection.mutable.ListBuffer`) / the new
  `mutable.Map[K, V]` and `mutable.Set[A]` (previously only `HashMap` / `HashSet`
  rode along; the `Map$` / `Set$` companions delegate at run time to `HashMap` /
  `HashSet` through `MapFactory$Delegate` / `IterableFactory$Delegate` while the
  static type stays the trait. `mutable.Map` has `apply` / `get` / `update` /
  `getOrElse` / `getOrElseUpdate` / `contains` / `keys` / `values` / `+=` / `-=` /
  `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `filter` /
  `toList` / `toSeq` / `iterator` / `mkString`; `mutable.Set` has `contains` /
  `+=` / `-=` / `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` /
  `map` / `filter` / `toList` / `toSeq` / `iterator` / `mkString`) /
  `StringBuilder` (`new` / `+=` / `append` / `toString` of
  `scala.collection.mutable.StringBuilder`) / `HashMap` (companion `empty` /
  varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` (`contains` / `foreach`, plus `+` /
  `-` / `++` / `size` / `isEmpty` / `nonEmpty` / `filter` / `map` / `toList` /
  `toSeq` / `iterator` / `mkString` / `head`) / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` / `isLeft` /
  `getOrElse` / `map`) / `Try` (`apply` / `map` / `getOrElse` on `Try$` /
  `Success` / `Failure`) / `Array$` (varargs `apply` + `ClassTag`). Dual-run:
  `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` /
  `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` /
  `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` /
  `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` /
  `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` /
  `view_bounds_class` / `hk_types` / `app` / `delayed_init` /
  `implicit_inherit_local` / `partial_function` / `list_collect` /
  `string_interp` / `overloading` / `classtag` / `context_bounds` /
  `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` /
  `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` /
  `pkg_implicit_class` / `structural_update` / `indexedseq_queue` /
  `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` /
  `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` /
  `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` /
  `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` /
  `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` /
  `custom_interp` / `array_ops`. **Still intrinsic / private, or not linked**:
  the rest of `StringOps`, the rest of the numerics, the other mutable
  collections. `List.unapplySeq` is `SeqOps`'s identity in the library. The
  varargs `apply` of `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library (what `agent/seqpat` added)**: `unapplySeq` on `Seq$` / `Vector$` /
  `IndexedSeq$` (an identity in practice; reads go through
  `lengthCompare$extension` / `apply$extension` / `drop$extension` on
  `scala/collection/SeqFactory$UnapplySeqWrapper$`) and `Array$.unapplySeq` (the
  same extensions on `scala/Array$UnapplySeqWrapper$`). `StringOps.map` becomes
  two methods: `Char => Char` goes to
  `map$extension(String, Function1)String` and everything else to
  `map$extension(String, Function1)IndexedSeq`. All of these are **library
  only**, and are diagnosed under `--no-scala-library`. Dual-run: `seqpat` /
  `seqpat_map` / `seqpat_ids` (`seqpat_ids` produces the same output on the
  private runtime too).
- **object**: as with scalac, a module `Main$` and a static forwarder `Main` are
  emitted. That is why `java Main` works.
- **Primitives**: `+` and friends on `Int` are emitted as JVM instructions
  (`iadd`, …), not as boxed methods on `scala.Int`.
- **traits**: a trait with only abstract members is a JVM interface. Concrete
  members become interface `default` methods plus instance forwarders in
  C3 linearization order. Java 8 default methods are not used. A `val` becomes
  getter/setter plus `$init$`. `abstract override` becomes `T$$super$m`.
- **Named arguments**: reordered at the call site, so `f(b = 2, a = 1)` works.
  There is no large rewrite phase. Reordering happens for methods, `apply`,
  `copy`, constructors and calls with overloads alike, and omitted default
  arguments are filled in on the spot (through a `{method}$default$n` getter for
  ordinary methods; for constructors the default expression is typed at the call
  site). Extractor patterns are reordered too, for case classes. The parser
  parses `x = e` uniformly as an assignment, and **it is the typer that treats
  one in argument position as a named argument** (the same construction as nsc).
- **try**: an exception table and a `StackMapTable` are emitted in the `Code`
  attribute.
- **Lambdas**: a plain `FunctionN` literal becomes **`invokedynamic` +
  `LambdaMetafactory`**, as in nsc 2.13, and no class file is emitted. The body
  becomes a `public static final synthetic $anonfun$N` in the enclosing class
  file, and captured values are the call site's arguments. **A `{ case }` where a
  `PartialFunction` is expected, and a position expecting a user-defined SAM
  type, are still synthetic classes** (`Main$$$anonfun$0` and the like).
  `PartialFunction` has two abstract methods, so it is not a SAM, and nsc emits a
  class file here too. When a synthetic class is used, locals of the enclosing
  method are captured into `$captured$n` fields and **the enclosing `this`** into
  an `$outer` field, the same as nsc. `this` is needed not only when it is
  written explicitly (`this.f` / `super.f`) but equally when the lambda merely
  **calls a method of the enclosing class** (`xs.map(a => base(a))`); members of
  an `object` do not need it, since they go through `MODULE$`. See "compiling
  lambdas to `invokedynamic`" for the details.
- **Phases**: there are no separate passes like nsc's mixin. There are
  **uncurry**, **lambda-lift** (nested defs), erasure, and the closure conversion
  of lambdas.
- **Method types in the pickle** (`ScalaSignature`). nsc's pickler runs *before*
  uncurry, so a class file records the parameter clauses the source wrote and
  keeps `def f: T` (a `NullaryMethodType`, pickled as a `POLYtpe` with no type
  parameters) apart from `def f(): T` (a `MethodType` over an empty list). Both
  halves of that matter to a reader: scalac accepts `f()` and `f` against the
  second and answers `T does not take parameters` to `f()` against the first.
  scala-rs pickles both distinctions the same way. Because our own uncurry has
  already joined a method's clauses by the time the backend sees the symbol,
  the shape is recorded on the symbol first (`Symbol::pickle_clauses`) and the
  pickler writes one `METHODtpe` per clause — so `def makeDatabase[F[_]: Async]()`
  reaches scalac as `[F[_]]()(implicit ev: Async[F])` rather than as
  `[F[_]](implicit ev: Async[F])`. A `name$default$n` getter with no clause
  before it stays *nullary*, as nsc's is; pickled with an empty list instead, a
  scalac caller that omits the argument compiles with "Auto-application to `()`
  is deprecated".

  The reading side is not symmetric yet. A jar (and any class a lazy completion
  reaches) goes through `crates/pickle`, which keeps every clause; the eager
  scan of a `-cp` **class directory** goes through the subset decoder in
  `crates/backend/src/pickle.rs`, which models a member as one flat parameter
  list. So `T.cur(1)(2)` against a class directory *we* produced is still
  rejected by scala-rs even though real scalac accepts the same class file.
- **sealed**: a non-exhaustive match is a warning, as in scalac. It becomes an
  error under `-Xfatal-warnings`.
- **The synthesized members of a `case class`** (SLS 5.3.2). `-Xprint:typer` on
  `case class Point(x: Int, y: String)` gives nsc's full set: `copy` /
  `copy$default$N` / `productPrefix` / `productArity` / `productElement` /
  `productIterator` / `canEqual` / `productElementName` / `hashCode` /
  `toString` / `equals`, and on the companion `toString` / `apply` / `unapply` /
  `writeReplace`. A `case object` gets the same minus `equals`, `copy` and
  `productElementName`. scala-rs emits all of those except the companion's
  `writeReplace` and the static forwarders nsc puts on the class for the
  companion's `apply` / `unapply` / `tupled` / `curried`; its case-accessor
  fields are `public final` where nsc's are `private final`.
  **`hashCode` is nsc's under `--scala-library` and the 31-fold under
  `--no-scala-library`.** It used to fold with 31 in both modes, so
  `Point(1, "a").hashCode` was `128` here and `-1322997830` under scalac:
  consistent with our own `equals`, but a case class we compile and one scalac
  compiles hashed differently, and any `HashMap`, `Map` or `Set` that saw both
  missed. Under `--scala-library` the emitted body is now nsc's, and nsc has
  *two* of them, chosen in `SyntheticMethods.chooseHashcode`:
  - **no case accessor has a primitive value type** (`Unit` counts) — a
    zero-field `case class Zero()`, `OnlyStr(s: String)`, `Cell[T](t: T)`,
    `AnyF(a: Any)`, `Arr(a: Array[Int])`, and `Box(m: Meters, b: String)` where
    `Meters extends AnyVal` — the whole method is
    `ScalaRunTime$.MODULE$._hashCode(this)`, i.e. `MurmurHash3.productHash`
    over `productArity` / `productElement`;
  - **otherwise** the mix chain is written out: `ldc` `MurmurHash3.productSeed`
    (`-889275714`), `Statics.mix` with `this.productPrefix.hashCode`, one `Int`
    per field, then `Statics.finalizeHash(h, arity)`. Per field: `Unit` / `Null`
    fold to the constant `0`, `Boolean` to `1231` / `1237` inline,
    `Int` / `Byte` / `Short` / `Char` are the value itself, `Long` / `Double` /
    `Float` go through `Statics.longHash` / `doubleHash` / `floatHash` (so
    `1.0.##` and `1.##` agree), and everything else through `Statics.anyHash`.
    A field of value-class type is stored unboxed but **hashed as an instance**:
    nsc emits `new Meters(this.m())` before `anyHash`, as `toString` and
    `productElement` also do.

  The two shapes agree by construction, so the split is bytecode fidelity
  rather than arithmetic; all 18 shapes checked, and 122 of slick's own case
  classes, disassemble instruction-for-instruction as scalac's do (modulo
  scala-rs reading case fields with `getfield` where nsc calls the accessor —
  the same divergence its `equals`, `toString` and `productElement` already
  have, and the same value).

  **Under `--no-scala-library` the 31-fold stays**, because
  `scala.runtime.Statics` and `scala.runtime.ScalaRunTime$` are library classes
  the private runtime does not have, and nothing in that mode ever meets a
  scalac-compiled case class. Its numbers are therefore *different by design*
  and pinned on their own
  (`tests/fixtures/expected/caseabi_hash_priv.txt` beside
  `caseabi_hash.txt`); what does not change between the modes is `hashCode`'s
  agreement with `equals`, which both expected files assert line for line. One
  consequence of the 31-fold that mode keeps: it reaches
  `java.util.Objects.hashCode` on the boxed field, so `AnyF(1.0)` and `AnyF(1)`
  hash *differently* there where Scala's `##` makes them agree.

  A `case object`'s `hashCode` was already nsc's (`"Foo".hashCode`) and is
  unchanged, in both modes.

  Still missing, and still on the list: the companion's `writeReplace`, and the
  static forwarders nsc puts on the class for the companion's `apply` /
  `unapply` / `tupled` / `curried`.
- **AnyVal**: scalac emits both the value class's class file and the extension
  methods. scala-rs does the same: `new C(x)` erases to the underlying value and
  calls go to the `$extension` static methods. In positions that need a reference
  (`Any` / a universal trait / a type argument / an array element) it boxes with
  `new C(u)` as nsc does, and synthesises `equals` / `hashCode` from the
  underlying value. The difference is where the `$extension` bodies live: nsc puts
  them in the companion `C$` and makes the class side a forwarder, while scala-rs
  emits them directly on the class.
- **Predef / StringOps**: on the private runtime there are `assert` / `require` /
  `???` / `->` (straight to `Tuple2`) / `identity` / `locally` / `implicitly` /
  `any2stringadd`, and `length`/`toInt`/`isEmpty` on `String`. In library mode:
  `Predef$.println/assert/require/???/identity/locally/implicitly`,
  `any2stringadd.$plus$extension`, `ArrowAssoc.$minus$greater$extension`,
  `intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension` /
  `until$extension`, `longWrapper` → `RichLong.abs$extension` / `max$extension` /
  `to` / `until` (`NumericRange$.inclusive` / `apply` +
  `Numeric$LongIsIntegral$`), `doubleWrapper` → `RichDouble.abs$extension` /
  `max$extension`, `floatWrapper` → `RichFloat.abs$extension` / `max$extension`,
  `charWrapper` → `RichChar.isDigit$extension` / `intValue$extension` (`.toInt`) /
  `to` / `until` (`NumericRange$.inclusive` / `apply` + `Numeric$CharIsIntegral$`),
  `byteWrapper` → `RichByte.abs$extension` / `max$extension` / `to` / `until`
  (`NumericRange$.inclusive` / `apply` + `Numeric$ByteIsIntegral$`),
  `shortWrapper` → `RichShort.max$extension` / `to` / `until`
  (`NumericRange$.inclusive` / `apply` + `Numeric$ShortIsIntegral$`),
  `booleanWrapper` → `RichBoolean.compare(Object)`, `augmentString` →
  `StringOps.toInt$extension` / `size$extension` (`.length`) /
  `$times$extension` / `take$extension` / `drop$extension` /
  `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` /
  `padTo$extension` (`Int, Char`) / `linesIterator$extension` /
  `toIntOption$extension` / `stripMargin$extension` / `lines$extension` /
  `capitalize$extension` / `reverse$extension` / `slice$extension` /
  `takeRight$extension` / `dropRight$extension` / `contains$extension`
  (`.isEmpty` / `.toUpperCase` / `.toLowerCase` are inlined through `StringOps`
  to `String`; `startsWith` / `endsWith` / `indexOf` go to `java.lang.String` as
  in nsc; also `head$extension` / `last$extension` / `stripLineEnd$extension` /
  `replaceAllLiterally$extension` / `tail$extension` / `init$extension` /
  `distinct$extension` / `mkString$extension` / `filter$extension` /
  `reverseIterator$extension`). `intArrayOps` → `ArrayOps.head$extension` /
  `tail$extension` / `foreach$extension(Object,Function1)V` /
  `map$extension(Object,Function1,ClassTag)Object`. `longArrayOps` → the same
  `head` / `foreach` (`[J]`). `refArrayOps` → `map` on reference arrays. **No
  `StringOps` / `ArrayOps` / `RichInt` / `RichLong` / `RichDouble` / `RichFloat` /
  `RichChar` / `RichByte` / `RichShort` / `RichBoolean` / `ArrayBuffer` /
  `ListBuffer` / `StringBuilder` / `HashMap` / `HashSet` / `LinkedHashMap` /
  `LinkedHashSet` / `ArrayDeque` / `NumericRange` class files are emitted.**
- **unapplySeq**: `List` / `Seq` / `Vector` / `IndexedSeq` / `Array` and
  user-defined extractors, `_*`, and named case class patterns. When linked
  against the library, `List.unapplySeq` returns `SeqOps`, and everything other
  than `List` indexes through `UnapplySeqWrapper`'s `$extension`, as in nsc.
  Sequence patterns on `Seq` / `Array` require jar linking (they are diagnosed
  under `--no-scala-library`).

- **Access diagnostics** are built the way nsc's `ContextErrors.AccessError`
  builds them: the subject is `underlyingSymbol(sym).fullLocationString`
  (`method x in class C`, `variable foo in class Sub2`, `object Inner in object
  Outer`), a module class is spelled `object C` rather than `C$`
  (`directObjectString`), the `from` clause names the enclosing class the same
  way (`object Use in package xflags`; nothing is appended for the empty
  package), and a constructor takes `in <owner>` in place of `as a member of
  <prefix>`. Two differences remain. The prefix type is not package-qualified
  — we print `object C` and `Holder` where nsc prints `object xflags.C` and
  `mism8bad.Holder` — which is `SymbolTable::display_type`'s doing and shows in
  every diagnostic, not only this one; and nsc appends an indented "Access to
  protected method `m` not permitted because …" explanation for the
  `protected` cases, which we do not print.

- **A `private` or `protected` constructor is access-checked** since
  `agent/intrinsicqual`. `agent/accessmsg` built nsc's `isClassConstructor`
  branch of that message and found it unreachable, and the reason was two
  gaps, not one: the primary constructor's symbol did not carry the modifier
  the class wrote (`class C private ()` puts it on `ctor_mods`, which the
  namer was not reading), and `new C(…)` is not a member selection, so
  `type_select`'s access check never saw an `<init>`. Both are closed.
  `new C(…)` now reports at nsc's line, with nsc's first line, on nsc's own
  `neg/sensitive`, `neg/t4987` and `neg/protected-constructors`.

  Four details, each measured rather than assumed:

  * The **prefix for a `new` is the class being constructed**, which is what
    nsc weighs a `protected` access against. So `class Sub extends Prot("x")`
    is legal — a parent-constructor call is not a `new` and never reaches the
    check — while `new Prot("x")` written inside `Sub` is not, and scalac
    2.13.16 agrees ("prefix type Prot does not conform to class Sub where the
    access takes place").
  * nsc drops inaccessible alternatives **before** overload resolution; our
    pick is made first. Where the class has another constructor this site may
    call, the check declines to speak rather than refuse a program scalac
    accepts (`class Mixed(val v: String) { private def this(len: Int) = … }`).
    The cost is that `neg/protected-constructors`' line 17 gets no diagnostic
    from us at all, where nsc reports an arity error against the accessible
    nullary constructor.
  * The check runs only on a `new` **the program wrote**. Several rewrites
    build one with `Tree::dummy`, and the access question there belongs to the
    member they rewrote: `v.copy(x = 2)` on a `case class C private (x: Int)`
    lowers to `new C(2)`, and nsc asks whether `copy` is accessible — which
    `case_copy_access_error` already does under
    `-Xsource-features:case-apply-copy-access` — never whether the constructor
    is. Reporting there refused `tests/fixtures/xflags_case_access_bad.scala`,
    which scalac compiles cleanly with no flag.
  * We still do not print nsc's indented "Access to protected constructor …
    not permitted because …" explanation, as for every other `protected` case.

  Putting the modifier on the constructor symbol also puts it in the
  `ScalaSignature`, which is where the change is visible: **four of slick's
  1490 class files differ, by exactly one byte each**
  (`slick/compiler/CompilerState`, `slick/basic/ConcurrencyControl` and its
  companion and `ConnectionArbiter` — the two slick classes with a `private`
  primary constructor), in the pickled flags of `<init>` and nowhere else. No
  bytecode, no method access flag and no constant-pool entry moves; the
  primary constructor is still emitted `ACC_PUBLIC`, as before. That the new
  byte is the right one has a direct measurement: **real scalac 2.13.16,
  reading our class file**, accepted `new SepPriv("x")` against the old pickle
  and refuses it against the new one with its own message — our signature had
  been telling it the constructor was public
  (`crates/cli/tests/intrinsicqual.rs`,
  `real_scalac_reads_the_constructor_as_private_from_our_classfile`).

- **`neg/t6601` is closed** by `agent/ctorgaps`, and it is the one of the four
  the check above does not reach on its own. It is a separate compilation:
  `PrivateConstructor_1` is compiled to a class file, and
  `AccessPrivateConstructor_2` reads it back. The class file cannot carry the
  answer — nsc emits even a `private` constructor `ACC_PUBLIC`, which `javap
  -p` on its own output confirms — so the `ScalaSignature` is the only record,
  and `PickleSupply::supply_ctors` was filtering constructors through
  `Member::is_public_api`, which *hides* a private member outright. The
  private `<init>` was dropped, the class file's public one stayed, and the
  call compiled. It is now supplied **and marked**: `install_ctor` copies
  `PRIVATE` / `PROTECTED` off the pickled symbol onto the constructor it
  repairs, and our diagnostic reproduces `neg/t6601.check` word for word —
  reading a class file we wrote and reading one real scalac wrote
  (`crates/cli/tests/ctorgaps.rs`).

  One case is deliberately left accessible: `private[p]` / `protected[p]`,
  which nsc pickles as the bare flag *plus* a `privateWithin` reference this
  reader does not resolve. Guessing "private" there would refuse every
  `private[slick]` constructor slick's own code calls, so a constructor with
  an access boundary keeps the accessibility it had before, and scalac refuses
  one program we accept. See `docs/not-implemented.md`.

It is not a replacement for scalac. It is a reimplementation of a subset.
